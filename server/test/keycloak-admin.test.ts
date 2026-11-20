import { describe, expect, it } from 'vitest'
import { HttpKeycloakAdmin, type SsoInput } from '../src/keycloak/admin.ts'

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

describe('upstream details stay out of problems (CWE-209)', () => {
  const internal = { apiUrl: 'http://kc.internal:8443/admin/realms/zeima', tokenUrl: 'http://kc.internal:8443/token', clientId: 'x', clientSecret: 'y' }
  const logged: unknown[] = []
  const log = { warn: (...a: unknown[]) => void logged.push(a) }
  const answering = (status: number, body: string): typeof fetch =>
    (async (url: string | URL | Request) => {
      if (String(url).endsWith('/token')) return new Response(JSON.stringify({ access_token: 't', expires_in: 300 }), { status: 200 })
      return new Response(body, { status })
    }) as typeof fetch
  const secret = 'SECRET-keycloak-text'

  it('an unreachable Keycloak is reported without its host or port, and logged', async () => {
    const down = (async () => {
      throw new Error('connect ECONNREFUSED kc.internal:8443')
    }) as typeof fetch
    const err = await new HttpKeycloakAdmin(internal, down, log).findUserByEmail('a@b.co').catch((e: unknown) => e)
    expect(err).toMatchObject({ status: 502, code: 'keycloak_unreachable' })
    expect((err as Error).message).not.toMatch(/kc\.internal|8443|ECONNREFUSED/)
    expect(JSON.stringify(logged)).toContain('cannot reach Keycloak')
  })

  it.each([400, 409, 500])('a %i from Keycloak keeps its status but not its text', async (status) => {
    const body = JSON.stringify({ errorMessage: secret, error_description: secret })
    const err = await new HttpKeycloakAdmin(internal, answering(status, body), log).createGroup('o', ['x']).catch((e: unknown) => e)
    expect(err).toMatchObject({ status: status === 500 ? 502 : status })
    expect((err as Error).message).not.toContain(secret)
    expect(JSON.stringify(logged)).toContain(secret)
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

  describe('SAML metadata', () => {
    const saml: SsoInput = { protocol: 'saml', name: 'Acme', enabled: true, sourceUrl: 'https://login.acme.example/metadata', domains: ['acme.example'] }
    const run = async (imported: Record<string, string>) => {
      const bodies: Record<string, unknown>[] = []
      const f = (async (url: string | URL | Request, init?: RequestInit) => {
        const u = String(url)
        if (u.endsWith('/token')) return new Response(JSON.stringify({ access_token: 't', expires_in: 300 }), { status: 200 })
        if (init?.method === 'POST' && u.endsWith('/identity-provider/instances')) bodies.push(JSON.parse(String(init.body)))
        if (u.includes('/identity-provider/instances/')) return new Response('', { status: 404 })
        return new Response('{}', { status: 200 })
      }) as typeof fetch
      await new HttpKeycloakAdmin(admin, f).setSso('o1', saml, imported)
      return (bodies[0] as { config: Record<string, string> }).config
    }

    it('keeps a signed-request requirement and the logout binding from the document', async () => {
      const config = await run({ singleSignOnServiceUrl: 'https://login.acme.example/sso', wantAuthnRequestsSigned: 'true', postBindingLogout: 'true', 'extra.option': 'x' })
      expect(config).toMatchObject({ wantAuthnRequestsSigned: 'true', postBindingLogout: 'true', validateSignature: 'true' })
      expect(config).not.toHaveProperty('extra.option')
    })

    it('never lets the document lower a setting that only ratchets up', async () => {
      for (const value of ['false', '', 'TRUE', '1']) {
        expect(await run({ wantAuthnRequestsSigned: value })).not.toHaveProperty('wantAuthnRequestsSigned')
      }
    })
  })
})
