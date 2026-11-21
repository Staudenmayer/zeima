import { describe, expect, it } from 'vitest'
import { gracefulShutdown } from '../src/shutdown.ts'
import { testApp } from './helpers.ts'

describe('health', () => {
  it('is alive without dependencies', async () => {
    const app = await testApp({ readiness: { keycloak: () => Promise.reject(new Error('down')) } })
    const res = await app.inject({ method: 'GET', url: '/healthz' })
    expect(res.json()).toEqual({ status: 'ok' })
    await app.close()
  })

  it('is ready when the database is, and reports Keycloak without depending on it', async () => {
    const up = await testApp({ readiness: { keycloak: async () => {} } })
    const ok = await up.inject({ method: 'GET', url: '/readyz' })
    expect(ok.statusCode).toBe(200)
    expect(ok.json()).toEqual({ status: 'ok', checks: {}, info: { keycloak: 'ok' } })
    await up.close()

    const kcDown = await testApp({ readiness: { keycloak: () => Promise.reject(new Error('down')) } })
    const still = await kcDown.inject({ method: 'GET', url: '/readyz' })
    expect(still.statusCode).toBe(200)
    expect(still.json()).toEqual({ status: 'ok', checks: {}, info: { keycloak: 'error' } })
    await kcDown.close()
  })

  it('is not ready when the database is not', async () => {
    const down = await testApp({
      db: {} as never,
      readiness: { keycloak: async () => {} },
      databaseCheck: () => Promise.reject(new Error('refused')),
    })
    const res = await down.inject({ method: 'GET', url: '/readyz' })
    expect(res.statusCode).toBe(503)
    expect(res.json()).toEqual({ status: 'unavailable', checks: { database: 'error' }, info: { keycloak: 'ok' } })
    await down.close()
  })

  it('answers 503 from the start of a shutdown, before anything is closed', async () => {
    const app = await testApp({ readiness: { keycloak: async () => {} } })
    expect((await app.inject({ method: 'GET', url: '/readyz' })).statusCode).toBe(200)
    const done = gracefulShutdown(app, { drainMs: 300 })
    await new Promise((r) => setTimeout(r, 50))
    const during = await app.inject({ method: 'GET', url: '/readyz' })
    expect(during.statusCode).toBe(503)
    expect(during.json().draining).toBe(true)
    // Liveness is unaffected, and requests still work while draining.
    expect((await app.inject({ method: 'GET', url: '/healthz' })).statusCode).toBe(200)
    await done
  })

  it('ends the pools after the server closes and waits for a running reconcile, but only briefly', async () => {
    const app = await testApp()
    const order: string[] = []
    let finish!: () => void
    const running = new Promise<void>((resolve) => (finish = resolve))
    const done = gracefulShutdown(app, {
      drainMs: 0,
      stopTimers: () => order.push('timers'),
      inFlight: () => running,
      inFlightLimitMs: 100,
      endPools: async () => void order.push('pools'),
    })
    await done // the run never finishes: the bound lets shutdown go on
    expect(order).toEqual(['timers', 'pools'])
    finish()
  })
})
