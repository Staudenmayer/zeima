import { describe, expect, it } from 'vitest'
import { testApp } from './helpers.ts'

describe('health', () => {
  it('is alive without dependencies', async () => {
    const app = await testApp({ readiness: { keycloak: () => Promise.reject(new Error('down')) } })
    const res = await app.inject({ method: 'GET', url: '/healthz' })
    expect(res.json()).toEqual({ status: 'ok' })
    await app.close()
  })

  it('is ready only when every dependency is', async () => {
    const up = await testApp({ readiness: { keycloak: async () => {} } })
    const ok = await up.inject({ method: 'GET', url: '/readyz' })
    expect(ok.statusCode).toBe(200)
    expect(ok.json()).toEqual({ status: 'ok', checks: { keycloak: 'ok' } })
    await up.close()

    const down = await testApp({
      readiness: { keycloak: async () => {}, database: () => Promise.reject(new Error('refused')) },
    })
    const res = await down.inject({ method: 'GET', url: '/readyz' })
    expect(res.statusCode).toBe(503)
    expect(res.json()).toEqual({ status: 'unavailable', checks: { keycloak: 'ok', database: 'error' } })
    await down.close()
  })
})
