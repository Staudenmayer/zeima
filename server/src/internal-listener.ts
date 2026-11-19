// A second listener for what only the installation itself may call: Keycloak's event listener
// (`/v1/internal/*`) and the metrics (`/metrics`). With `INTERNAL_PORT` set, those paths answer on that port only
// and are 404 on the public one, so a reverse proxy or ingress that forwards everything on the public port can
// never publish them. Without it (development) they stay on the public port.

import http from 'node:http'
import type { FastifyInstance } from 'fastify'

export const INTERNAL_PATHS = ['/v1/internal/', '/metrics']

/** Whether a path belongs to the internal surface. */
export function isInternalPath(url: string): boolean {
  const path = url.split('?')[0] ?? ''
  return INTERNAL_PATHS.some((p) => (p.endsWith('/') ? path.startsWith(p) : path === p))
}

/** Refuses the internal paths on every port but the internal one. */
export function guardInternalPaths(app: FastifyInstance, internalPort: number | null): void {
  if (internalPort === null) return
  app.addHook('onRequest', async (request, reply) => {
    if (isInternalPath(request.url) && request.raw.socket.localPort !== internalPort) {
      return reply.code(404).type('application/problem+json').send({ type: 'about:blank', title: 'Not Found', status: 404, code: 'not_found' })
    }
  })
}

/** Serves the same app on a second port (the internal one). The caller closes the returned server on shutdown. */
export function listenInternal(app: FastifyInstance, port: number, host: string): Promise<http.Server> {
  const server = http.createServer((req, res) => app.routing(req, res))
  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(port, host, () => resolve(server))
  })
}
