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

export const healthRoutes: FastifyPluginAsyncTypebox<{ checks: Record<string, ReadinessCheck>; cacheMs?: number }> = async (
  app,
  { checks, cacheMs = 5_000 },
) => {
  app.get(
    '/healthz',
    {
      config: { public: 'liveness probe' },
      schema: {
        tags: ['health'],
        summary: 'Liveness: the process is running',
        response: { 200: Type.Object({ status: Type.Literal('ok') }) },
      },
    },
    async () => ({ status: 'ok' as const }),
  )

  // One round of checks serves every caller for a few seconds: the endpoint is public and each round calls Keycloak and the database.
  let cached: { at: number; ok: boolean; checks: Record<string, 'ok' | 'error'> } | null = null

  app.get(
    '/readyz',
    {
      config: { public: 'readiness probe' },
      schema: {
        tags: ['health'],
        summary: 'Readiness: the dependencies (Keycloak, the database) are reachable',
        response: { 200: Readiness, 503: Readiness },
      },
    },
    async (request, reply) => {
      if (cached && Date.now() - cached.at < cacheMs) {
        return reply.code(cached.ok ? 200 : 503).send({ status: cached.ok ? 'ok' : 'unavailable', checks: cached.checks })
      }
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
      cached = { at: Date.now(), ok, checks: Object.fromEntries(results) }
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
