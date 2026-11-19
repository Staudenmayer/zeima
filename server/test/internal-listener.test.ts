import net from 'node:net'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { isInternalPath, listenInternal } from '../src/internal-listener.ts'
import { keys } from './helpers.ts'

const freePort = () =>
  new Promise<number>((resolve) => {
    const s = net.createServer().listen(0, '127.0.0.1', () => {
      const { port } = s.address() as net.AddressInfo
      s.close(() => resolve(port))
    })
  })

describe('what is internal', () => {
  it('is the Keycloak events and the metrics, however the path is spelled with a query', () => {
    expect(isInternalPath('/v1/internal/keycloak-events')).toBe(true)
    expect(isInternalPath('/v1/internal/anything?x=1')).toBe(true)
    expect(isInternalPath('/metrics')).toBe(true)
    expect(isInternalPath('/metrics?format=text')).toBe(true)
    expect(isInternalPath('/v1/me')).toBe(false)
    expect(isInternalPath('/v1/internalish')).toBe(false)
    expect(isInternalPath('/metricsx')).toBe(false)
  })
})

describe('the internal port', () => {
  let app: Awaited<ReturnType<typeof buildApp>>
  let publicPort = 0
  let internalPort = 0
  let internalServer: Awaited<ReturnType<typeof listenInternal>>

  beforeAll(async () => {
    internalPort = await freePort()
    app = await buildApp(
      loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'true', KEYCLOAK_WEBHOOK_SECRET: 'a-long-enough-secret', INTERNAL_PORT: String(internalPort) }),
      { logger: false, keys, readiness: {} },
    )
    await app.listen({ host: '127.0.0.1', port: 0 })
    publicPort = (app.server.address() as net.AddressInfo).port
    internalServer = await listenInternal(app, internalPort, '127.0.0.1')
  })
  afterAll(async () => {
    await new Promise<void>((resolve) => internalServer.close(() => resolve()))
    await app?.close()
  })

  const get = (port: number, path: string, method = 'GET') => fetch(`http://127.0.0.1:${port}${path}`, { method, headers: { 'content-type': 'application/json' }, body: method === 'POST' ? '{}' : undefined })

  it('is the only place the metrics are served', async () => {
    expect((await get(publicPort, '/metrics')).status).toBe(404)
    expect((await get(internalPort, '/metrics')).status).toBe(200)
  })

  it('is the only place Keycloak\'s events are accepted', async () => {
    expect((await get(publicPort, '/v1/internal/keycloak-events', 'POST')).status).toBe(404)
    // On the internal port the route exists (this call has no signature, so it is refused as unauthorized, not as missing).
    expect((await get(internalPort, '/v1/internal/keycloak-events', 'POST')).status).toBe(400)
  })

  it('leaves the rest of the API on the public port', async () => {
    expect((await get(publicPort, '/healthz')).status).toBe(200)
    expect((await get(publicPort, '/.well-known/zeima-server')).status).toBe(200)
  })
})

describe('without an internal port (development)', () => {
  it('serves the internal paths on the public port, as before', async () => {
    const app = await buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'true' }), { logger: false, keys, readiness: {} })
    expect((await app.inject({ url: '/metrics' })).statusCode).toBe(200)
    await app.close()
  })
})
