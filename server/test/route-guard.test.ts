import { describe, expect, it } from 'vitest'
import { loadConfig } from '../src/config.ts'
import { testApp } from './helpers.ts'

describe('routes that forget to say how they are protected', () => {
  it('stop the server from starting', async () => {
    const app = await testApp()
    expect(() => app.get('/oops', async () => 'open to the world')).toThrow(/neither protected .* nor marked public/)
    await app.close()
  })

  it('are fine when they declare a security scheme or say why they are public', async () => {
    const app = await testApp()
    expect(() => app.get('/fine-public', { config: { public: 'a test' } }, async () => 'ok')).not.toThrow()
    expect(() => app.get('/fine-secure', { schema: { security: [{ keycloak: [] }] } }, async () => 'ok')).not.toThrow()
    await app.close()
  })
})

describe('defaults that fail closed', () => {
  it('are production, no API docs and no metrics unless switched on', () => {
    const c = loadConfig({})
    expect(c.env).toBe('production')
    expect(c.docsEnabled).toBe(false)
    expect(c.metricsEnabled).toBe(false)
  })
})

import { parseTrustProxy } from '../src/config.ts'
import { Quota } from '../src/plugins/quota.ts'

describe('TRUST_PROXY', () => {
  it('takes a number of proxies, a list of ranges, or on/off', () => {
    expect(parseTrustProxy('false')).toBe(false)
    expect(parseTrustProxy('')).toBe(false)
    expect(parseTrustProxy('true')).toBe(true)
    expect(parseTrustProxy('1')).toBe(1)
    expect(parseTrustProxy('10.0.0.0/8, 172.16.0.0/12')).toEqual(['10.0.0.0/8', '172.16.0.0/12'])
    expect(parseTrustProxy('loopback')).toEqual(['loopback'])
    expect(() => parseTrustProxy('yes please')).toThrow(/TRUST_PROXY/)
  })

  it('counts hops (as buildApp does): with one proxy the caller cannot choose their own address', async () => {
    const { default: Fastify } = await import('fastify')
    const app = Fastify({ trustProxy: (_address: string, hop: number) => hop < 1 })
    app.get('/ip', async (request) => request.ip)
    // Behind one proxy that appends the real address: the caller sent a forged first entry.
    const res = await app.inject({ url: '/ip', headers: { 'x-forwarded-for': '6.6.6.6, 203.0.113.9' }, remoteAddress: '10.0.0.2' })
    expect(res.body).toBe('203.0.113.9')
    await app.close()
  })
})

describe('per-account budgets', () => {
  it('count over a window and start again', () => {
    const q = new Quota(3, 1000)
    expect(q.take('a', 1, 0)).toBe(true)
    expect(q.take('a', 2, 10)).toBe(true)
    expect(q.take('a', 1, 20)).toBe(false)
    expect(q.take('b', 3, 20)).toBe(true) // another account has its own budget
    expect(q.take('a', 1, 1001)).toBe(true) // a new window
  })

  it('throw the 429 clients know how to wait out', () => {
    const q = new Quota(1, 60_000)
    q.spend('a')
    expect(() => q.spend('a')).toThrow(expect.objectContaining({ status: 429, code: 'quota_exceeded' }))
  })
})
