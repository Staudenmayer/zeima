// Liveness (/healthz) and readiness (/readyz) for orchestrators (SPEC §7.1).

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'

/** A dependency check; throws when the dependency is not usable. */
export type ReadinessCheck = () => Promise<void>

const Status = Type.Union([Type.Literal('ok'), Type.Literal('error')])

const Readiness = Type.Object({
  status: Type.Union([Type.Literal('ok'), Type.Literal('unavailable')]),
  checks: Type.Record(Type.String(), Status),
  /** Dependencies that are reported but never make the probe fail (Keycloak: a replica without it still serves sync). */
  info: Type.Optional(Type.Record(Type.String(), Status)),
  /** Present (true) while the process is shutting down. */
  draining: Type.Optional(Type.Boolean()),
})

export const healthRoutes: FastifyPluginAsyncTypebox<{ checks: Record<string, ReadinessCheck>; info?: Record<string, ReadinessCheck>; cacheMs?: number }> = async (
  app,
  { checks, info = {}, cacheMs = 5_000 },
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
  let cached: { at: number; ok: boolean; checks: Record<string, 'ok' | 'error'>; info: Record<string, 'ok' | 'error'> } | null = null

  app.get(
    '/readyz',
    {
      config: { public: 'readiness probe' },
      schema: {
        tags: ['health'],
        summary: 'Readiness: the database is reachable and the process is not shutting down (Keycloak is reported, not required)',
        response: { 200: Readiness, 503: Readiness },
      },
    },
    async (request, reply) => {
      const body = (ok: boolean, c: Record<string, 'ok' | 'error'>, inf: Record<string, 'ok' | 'error'>, draining: boolean) => ({
        status: ok ? ('ok' as const) : ('unavailable' as const),
        checks: c,
        ...(Object.keys(inf).length > 0 && { info: inf }),
        ...(draining && { draining: true }),
      })
      // Shutting down: tell the load balancer at once, whatever the cache says.
      if (app.lifecycle.draining) return reply.code(503).send(body(false, cached?.checks ?? {}, cached?.info ?? {}, true))
      if (cached && Date.now() - cached.at < cacheMs) return reply.code(cached.ok ? 200 : 503).send(body(cached.ok, cached.checks, cached.info, false))
      const run = (set: Record<string, ReadinessCheck>) =>
        Promise.all(
          Object.entries(set).map(async ([name, check]) => {
            try {
              await check()
              return [name, 'ok'] as const
            } catch (err) {
              request.log.warn({ err, check: name }, 'readiness check failed')
              return [name, 'error'] as const
            }
          }),
        )
      const [results, infoResults] = await Promise.all([run(checks), run(info)])
      const ok = results.every(([, s]) => s === 'ok')
      cached = { at: Date.now(), ok, checks: Object.fromEntries(results), info: Object.fromEntries(infoResults) }
      return reply.code(ok ? 200 : 503).send(body(ok, cached.checks, cached.info, false))
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
