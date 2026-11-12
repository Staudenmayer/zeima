// Liveness (/healthz) and readiness (/readyz) for orchestrators (SPEC §7.1).

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'

/** A dependency check; throws when the dependency is not usable. */
export type ReadinessCheck = () => Promise<void>

const Status = Type.Union([Type.Literal('ok'), Type.Literal('error')])

const Readiness = Type.Object({
  status: Type.Union([Type.Literal('ok'), Type.Literal('unavailable')]),
  checks: Type.Record(Type.String(), Status),
})

export const healthRoutes: FastifyPluginAsyncTypebox<{ checks: Record<string, ReadinessCheck> }> = async (
  app,
  { checks },
) => {
  app.get(
    '/healthz',
    {
      schema: {
        tags: ['health'],
        summary: 'Liveness: the process is running',
        response: { 200: Type.Object({ status: Type.Literal('ok') }) },
      },
    },
    async () => ({ status: 'ok' as const }),
  )

  app.get(
    '/readyz',
    {
      schema: {
        tags: ['health'],
        summary: 'Readiness: the dependencies (Keycloak, later the database) are reachable',
        response: { 200: Readiness, 503: Readiness },
      },
    },
    async (request, reply) => {
      const results = await Promise.all(
        Object.entries(checks).map(async ([name, check]) => {
          try {
            await check()
            return [name, 'ok'] as const
          } catch (err) {
            request.log.warn({ err, check: name }, 'readiness check failed')
            return [name, 'error'] as const
          }
        }),
      )
      const ok = results.every(([, s]) => s === 'ok')
      return reply.code(ok ? 200 : 503).send({ status: ok ? 'ok' : 'unavailable', checks: Object.fromEntries(results) })
    },
  )
}

/** Keycloak is ready when the realm's signing keys can be fetched. */
export function keycloakCheck(jwksUrl: string): ReadinessCheck {
  return async () => {
    const response = await fetch(jwksUrl, { signal: AbortSignal.timeout(2_000) })
    if (!response.ok) throw new Error(`JWKS answered ${response.status}`)
  }
}
