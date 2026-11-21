// buildApp(config): the whole API without listening, for the server entry
// point, tests (`app.inject`) and the OpenAPI export.

import { guardInternalPaths } from './internal-listener.ts'
import type { TypeBoxTypeProvider } from '@fastify/type-provider-typebox'
import { sql } from 'drizzle-orm'
import Fastify, { type FastifyServerOptions } from 'fastify'
import type { JWTVerifyGetKey } from 'jose'
import type { Config } from './config.ts'
import type { Db } from './db/index.ts'
import { Licensing } from './licensing/service.ts'
import { discoveryRoutes } from './modules/discovery/routes.ts'
import { healthRoutes, keycloakCheck, type ReadinessCheck } from './modules/health/routes.ts'
import { billingRoutes } from './modules/billing/routes.ts'
import { licenseRoutes, licensingRoutes } from './modules/licensing/routes.ts'
import { internalRoutes } from './modules/internal/routes.ts'
import { orgRoutes } from './modules/orgs/routes.ts'
import { addonRoutes } from './modules/addons/routes.ts'
import { jurisdictionRoutes } from './modules/jurisdictions/routes.ts'
import { scimRoutes } from './modules/orgs/scim.ts'
import { directoryRoutes } from './modules/directory/routes.ts'
import { ssoRoutes } from './modules/orgs/sso.ts'
import { HttpKeycloakAdmin, type KeycloakAdmin } from './keycloak/admin.ts'
import { syncRoutes } from './modules/sync/routes.ts'
import { meRoutes } from './modules/me/routes.ts'
import { authPlugin } from './plugins/auth.ts'
import { rolesPlugin } from './modules/orgs/roles.ts'
import { realtimePlugin } from './plugins/realtime.ts'
import { identityPlugin } from './plugins/identity.ts'
import { docsPlugin } from './plugins/docs.ts'
import { errorsPlugin } from './plugins/errors.ts'
import { metricsPlugin } from './plugins/metrics.ts'
import { securityPlugin } from './plugins/security.ts'

export interface AppOptions {
  logger?: FastifyServerOptions['logger']
  /** Token signing keys (tests); default: the realm's JWKS. */
  keys?: JWTVerifyGetKey
  /** Database for the routes; adds a readiness check. Omit for no database. */
  db?: Db
  /** Keycloak admin client (tests pass a fake); default: HTTP when the config has admin credentials. */
  keycloakAdmin?: KeycloakAdmin | null
  /** Register the modules of both deployment modes. For the API document only. */
  documentAll?: boolean
  /** Readiness checks; default: Keycloak. */
  readiness?: Record<string, ReadinessCheck>
  /** TXT lookups for the proof of an email domain (tests pass their own); default: the DNS. */
  dnsTxt?: (name: string) => Promise<string[][]>
  /** Host name lookups for the safety check of a provider's address (tests pass their own). */
  dnsResolver?: (host: string) => Promise<{ address: string }[]>
  /** Reads a single sign-on provider's metadata document; the real network unless a test gives its own. */
  ssoFetch?: (url: URL) => Promise<string>
}

export async function buildApp(config: Config, options: AppOptions = {}) {
  const app = Fastify({
    logger: options.logger ?? {
      level: config.logLevel,
      redact: ['req.headers.authorization', 'req.headers.cookie'],
    },
    // A number is how many proxies are in front: only that many hops from the socket are believed.
    trustProxy: typeof config.trustProxy === 'number' ? (_address: string, hop: number) => hop < (config.trustProxy as number) : config.trustProxy,
    // Clients may send ids generated offline; keep them unchanged.
    ajv: { customOptions: { removeAdditional: false, coerceTypes: 'array', useDefaults: true } },
    bodyLimit: 5 * 1024 * 1024,
  }).withTypeProvider<TypeBoxTypeProvider>()

  // Authentication is declared per route (`schema.security`). A route that declares neither that nor
  // `config: { public: '<why>' }` is a mistake that would silently expose it: the server does not start.
  app.addHook('onRoute', (route) => {
    const methods = [route.method].flat()
    if (methods.every((m) => m === 'HEAD' || m === 'OPTIONS')) return
    const schema = route.schema as { security?: unknown[] } | undefined
    const marked = (route.config as { public?: string } | undefined)?.public
    if (schema?.security?.length || marked || route.url === '/docs' || route.url.startsWith('/docs/')) return
    throw new Error(`route ${methods.join(',')} ${route.url} is neither protected (schema.security) nor marked public (config.public)`)
  })

  guardInternalPaths(app, config.internalPort)
  await app.register(errorsPlugin)
  await app.register(securityPlugin, { config })
  // Before the routes: it collects them through onRoute.
  await app.register(docsPlugin, { config })
  await app.register(authPlugin, { config, keys: options.keys })
  await app.register(identityPlugin)
  await app.register(realtimePlugin)
  if (config.metricsEnabled) await app.register(metricsPlugin)

  const { db } = options
  app.decorate('licensing', new Licensing(config))
  app.decorate('keycloakAdmin', options.keycloakAdmin !== undefined ? options.keycloakAdmin : config.keycloak.admin ? new HttpKeycloakAdmin(config.keycloak.admin, fetch, app.log) : null)
  if (db) app.decorate('db', db)
  await app.register(rolesPlugin, { membershipTtlMs: config.env === 'test' ? 0 : undefined })
  await app.register(healthRoutes, {
    cacheMs: config.env === 'test' ? 0 : undefined,
    checks: {
      ...(options.readiness ?? { keycloak: keycloakCheck(config.keycloak.jwksUrl) }),
      ...(db && { database: async () => void (await db.execute(sql`select 1`)) }),
    },
  })
  await app.register(discoveryRoutes, { config })
  await app.register(
    async (v1) => {
      await v1.register(meRoutes)
      await v1.register(orgRoutes)
      await v1.register(ssoRoutes, { config, ...(options.dnsTxt && { txt: options.dnsTxt }), ...(options.dnsResolver && { resolver: options.dnsResolver }), ...(options.ssoFetch && { fetchMetadata: options.ssoFetch }) })
      await v1.register(scimRoutes, { config })
      await v1.register(addonRoutes, { config })
      await v1.register(jurisdictionRoutes, { config })
      await v1.register(internalRoutes, { config })
      await v1.register(syncRoutes)
      await v1.register(licensingRoutes)
      // Offline licenses are for self-hosted installs; the cloud has billing.
      if (config.deploymentMode === 'self_hosted' || options.documentAll) await v1.register(licenseRoutes)
      // A company directory (LDAP / Active Directory) is for installations that run their own Keycloak.
      if (config.deploymentMode === 'self_hosted' || options.documentAll) await v1.register(directoryRoutes)
      // Purchases exist only in the cloud; self-hosted installs use license files.
      if (config.deploymentMode === 'cloud' || options.documentAll) await v1.register(billingRoutes, { config })
    },
    { prefix: '/v1' },
  )
  return app
}

declare module 'fastify' {
  interface FastifyContextConfig {
    /** Marks a route that is open on purpose, and says why (see the `onRoute` check in `buildApp`). */
    public?: string
    /** Served on the internal port only (see internal-listener.ts). */
    internal?: boolean
  }
}

export type App = Awaited<ReturnType<typeof buildApp>>

declare module 'fastify' {
  interface FastifyInstance {
    /** Present when the app was built with a database. */
    db: Db
    licensing: Licensing
    /** Null when no admin credentials are configured: organization management is off. */
    keycloakAdmin: KeycloakAdmin | null
  }
}
