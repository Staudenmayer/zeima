import { describe, expect, it } from 'vitest'
import { HttpKeycloakAdmin } from '../src/keycloak/admin.ts'

const admin = { apiUrl: 'http://kc/admin/realms/zeima', tokenUrl: 'http://kc/token', clientId: 'x', clientSecret: 'y' }
const fakeFetch = (status: number): typeof fetch =>
  (async (url: string | URL | Request) => {
    if (String(url).endsWith('/token')) return new Response(JSON.stringify({ access_token: 't', expires_in: 300 }), { status: 200 })
    return new Response('{}', { status })
  }) as typeof fetch

describe('Keycloak answers', () => {
  it('403 is told apart from other failures: the service account lacks a role', async () => {
    await expect(new HttpKeycloakAdmin(admin, fakeFetch(403)).findUserByEmail('a@b.co')).rejects.toMatchObject({ status: 502, code: 'keycloak_forbidden' })
  })

  it('keeps the other statuses as they were', async () => {
    await expect(new HttpKeycloakAdmin(admin, fakeFetch(404)).deleteOrganization('x')).rejects.toMatchObject({ status: 404 })
    await expect(new HttpKeycloakAdmin(admin, fakeFetch(409)).createGroup('o', ['x'])).rejects.toBeDefined()
  })
})

describe('setSso', () => {
  it('keeps the addresses and certificates of an imported document and drops its other settings', async () => {
    const bodies: Record<string, unknown>[] = []
    const f = (async (url: string | URL | Request, init?: RequestInit) => {
      const u = String(url)
      if (u.endsWith('/token')) return new Response(JSON.stringify({ access_token: 't', expires_in: 300 }), { status: 200 })
      if (init?.method === 'POST' && u.endsWith('/identity-provider/instances')) bodies.push(JSON.parse(String(init.body)))
      if (u.includes('/organizations/o1') && !init?.method) return new Response('{}', { status: 200 })
      if (u.includes('/identity-provider/instances/')) return new Response('', { status: 404 })
      return new Response('{}', { status: 200 })
    }) as typeof fetch
    await new HttpKeycloakAdmin(admin, f).setSso(
      'o1',
      { protocol: 'oidc', name: 'Acme', enabled: true, sourceUrl: 'https://login.acme.example/.well-known/openid-configuration', domains: ['acme.example'], clientId: 'c', clientSecret: 's' },
      { issuer: 'https://login.acme.example', tokenUrl: 'https://login.acme.example/token', jwksUrl: 'https://login.acme.example/keys', hideOnLogin: 'false', validateSignature: 'false', trustEmail: 'true', 'extra.option': 'x' },
    )
    const config = (bodies[0] as { config: Record<string, string> }).config
    expect(config).toMatchObject({ issuer: 'https://login.acme.example', tokenUrl: 'https://login.acme.example/token', jwksUrl: 'https://login.acme.example/keys', validateSignature: 'true', syncMode: 'IMPORT' })
    expect(config).not.toHaveProperty('hideOnLogin')
    expect(config).not.toHaveProperty('trustEmail')
    expect(config).not.toHaveProperty('extra.option')
  })
})
