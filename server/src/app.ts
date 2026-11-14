// buildApp(config): the whole API without listening, for the server entry
// point, tests (`app.inject`) and the OpenAPI export.

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
}

export async function buildApp(config: Config, options: AppOptions = {}) {
  const app = Fastify({
    logger: options.logger ?? {
      level: config.logLevel,
      redact: ['req.headers.authorization', 'req.headers.cookie'],
    },
    trustProxy: config.trustProxy,
    // Clients may send ids generated offline; keep them unchanged.
    ajv: { customOptions: { removeAdditional: false, coerceTypes: 'array', useDefaults: true } },
    bodyLimit: 5 * 1024 * 1024,
  }).withTypeProvider<TypeBoxTypeProvider>()

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
  app.decorate('keycloakAdmin', options.keycloakAdmin !== undefined ? options.keycloakAdmin : config.keycloak.admin ? new HttpKeycloakAdmin(config.keycloak.admin) : null)
  if (db) app.decorate('db', db)
  await app.register(rolesPlugin)
  await app.register(healthRoutes, {
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
      await v1.register(internalRoutes, { config })
      await v1.register(syncRoutes)
      await v1.register(licensingRoutes)
      // Offline licenses are for self-hosted installs; the cloud has billing.
      if (config.deploymentMode === 'self_hosted' || options.documentAll) await v1.register(licenseRoutes)
      // Purchases exist only in the cloud; self-hosted installs use license files.
      if (config.deploymentMode === 'cloud' || options.documentAll) await v1.register(billingRoutes, { config })
    },
    { prefix: '/v1' },
  )
  return app
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
