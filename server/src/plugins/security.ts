// HTTP hardening: security headers, CORS, rate limits and load shedding.

import cors from '@fastify/cors'
import helmet from '@fastify/helmet'
import rateLimit from '@fastify/rate-limit'
import underPressure from '@fastify/under-pressure'
import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import type { Config } from '../config.ts'
import { HttpProblem } from './errors.ts'

export const securityPlugin = fp(
  async (app: FastifyInstance, { config }: { config: Config }) => {
    // The Swagger UI sets its own CSP (staticCSP); JSON needs none.
    await app.register(helmet, { contentSecurityPolicy: false })
    await app.register(cors, {
      origin: config.corsOrigins.length ? config.corsOrigins : false,
      credentials: false,
    })
    await app.register(rateLimit, {
      max: config.rateLimitPerMinute,
      timeWindow: '1 minute',
      // Injected test sockets have no address.
      keyGenerator: (request) => request.ip ?? 'unknown',
      // Probes must keep working under load.
      allowList: (request) => request.url === '/healthz' || request.url === '/readyz',
      errorResponseBuilder: (_request, context) =>
        new HttpProblem(429, 'rate_limited', `too many requests; try again in ${context.after}`),
    })
    await app.register(underPressure, {
      maxEventLoopDelay: 1_000,
      maxHeapUsedBytes: 0,
      maxRssBytes: 0,
      exposeStatusRoute: false,
      pressureHandler: () => {
        throw new HttpProblem(503, 'overloaded', 'the server is overloaded; try again shortly', { 'retry-after': '5' })
      },
    })
  },
  { name: 'security', dependencies: ['errors'] },
)
