// buildApp(config): the whole API without listening, for the server entry
// point, tests (`app.inject`) and the OpenAPI export.

import type { TypeBoxTypeProvider } from '@fastify/type-provider-typebox'
import Fastify, { type FastifyServerOptions } from 'fastify'
import type { JWTVerifyGetKey } from 'jose'
import type { Config } from './config.ts'
import { healthRoutes, keycloakCheck, type ReadinessCheck } from './modules/health/routes.ts'
import { meRoutes } from './modules/me/routes.ts'
import { authPlugin } from './plugins/auth.ts'
import { docsPlugin } from './plugins/docs.ts'
import { errorsPlugin } from './plugins/errors.ts'
import { metricsPlugin } from './plugins/metrics.ts'
import { securityPlugin } from './plugins/security.ts'

export interface AppOptions {
  logger?: FastifyServerOptions['logger']
  /** Token signing keys (tests); default: the realm's JWKS. */
  keys?: JWTVerifyGetKey
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
  if (config.metricsEnabled) await app.register(metricsPlugin)

  await app.register(healthRoutes, {
    checks: options.readiness ?? { keycloak: keycloakCheck(config.keycloak.jwksUrl) },
  })
  await app.register(
    async (v1) => {
      await v1.register(meRoutes)
    },
    { prefix: '/v1' },
  )
  return app
}

export type App = Awaited<ReturnType<typeof buildApp>>
