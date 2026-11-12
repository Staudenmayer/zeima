// Prometheus metrics at /metrics: process defaults and request durations.
// Not in the API docs; restrict access to it at the reverse proxy.

import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { collectDefaultMetrics, Histogram, Registry } from 'prom-client'

export const metricsPlugin = fp(
  async (app: FastifyInstance) => {
    const registry = new Registry()
    collectDefaultMetrics({ register: registry })
    const duration = new Histogram({
      name: 'http_request_duration_seconds',
      help: 'HTTP request duration by route',
      labelNames: ['method', 'route', 'status'],
      buckets: [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5],
      registers: [registry],
    })

    app.addHook('onResponse', async (request, reply) => {
      // Route patterns, not URLs, so ids do not create new series.
      const route = request.routeOptions.url ?? 'unmatched'
      if (route === '/metrics') return
      duration.observe({ method: request.method, route, status: String(reply.statusCode) }, reply.elapsedTime / 1000)
    })

    app.get('/metrics', { schema: { hide: true } }, async (_request, reply) =>
      reply.type(registry.contentType).send(await registry.metrics()),
    )
  },
  { name: 'metrics' },
)
