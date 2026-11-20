// A second listener for what only the installation itself may call: Keycloak's event listener
// (`/v1/internal/*`) and the metrics (`/metrics`). With `INTERNAL_PORT` set, those paths answer on that port only
// and are 404 on the public one, so a reverse proxy or ingress that forwards everything on the public port can
// never publish them. Without it (development) they stay on the public port.

import http from 'node:http'
import type { FastifyInstance } from 'fastify'

export const INTERNAL_PATHS = ['/v1/internal/', '/metrics']

/** The path as the router will read it: percent-decoded, lower-case, dots and repeated slashes resolved. */
function normalizedPath(url: string): string {
  let path = url.split('?')[0]!.split(';')[0]!
  for (let i = 0; i < 3; i++) {
    try {
      const next = decodeURIComponent(path)
      if (next === path) break
      path = next
    } catch {
      break
    }
  }
  const parts: string[] = []
  for (const part of path.toLowerCase().split('/')) {
    if (part === '' || part === '.') continue
    if (part === '..') parts.pop()
    else parts.push(part)
  }
  return `/${parts.join('/')}`
}

/** Whether a path belongs to the internal surface (in any spelling the router would accept). */
export function isInternalPath(url: string): boolean {
  const raw = url.split('?')[0] ?? ''
  const path = normalizedPath(url)
  return INTERNAL_PATHS.some((p) => {
    const bare = p.replace(/\/$/, '')
    return p.endsWith('/') ? path === bare || path.startsWith(p) || raw.startsWith(p) : path === p || raw === p
  })
}

/** Refuses the internal paths on every port but the internal one. */
export function guardInternalPaths(app: FastifyInstance, internalPort: number | null): void {
  if (internalPort === null) return
  app.addHook('onRequest', async (request, reply) => {
    // By the route (already matched by the router, whatever the spelling) and by the normalised path.
    const internal = request.routeOptions?.config?.internal === true || isInternalPath(request.url)
    const onInternalPort = request.raw.socket.localPort === internalPort
    // The internal port serves the internal routes only: published by mistake, it must not be a second way into the API.
    const refused = onInternalPort ? request.routeOptions?.config?.internal !== true : internal
    if (refused) {
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
