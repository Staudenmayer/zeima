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
