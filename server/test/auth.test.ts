import { generateKeyPair } from 'jose'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import type { App } from '../src/app.ts'
import { orgsFromGroups } from '../src/plugins/auth.ts'
import { defaultPermissions } from '../src/plugins/permissions.ts'
import { testApp, token } from './helpers.ts'

describe('orgsFromGroups', () => {
  it('reads roles and departments from group paths', () => {
    expect(
      orgsFromGroups([
        '/orgs/o1/roles/member',
        '/orgs/o1/roles/manager',
        '/orgs/o1/groups/dev/backend',
        '/orgs/o2/groups/sales',
        '/orgs/o3/roles/emperor',
        '/orgs/o4',
        '/other/group',
      ]),
    ).toEqual({
      o1: { role: 'manager', permissions: defaultPermissions('manager'), groups: ['/orgs/o1/groups/dev/backend'] },
      o2: { role: 'member', permissions: defaultPermissions('member'), groups: ['/orgs/o2/groups/sales'] },
      // A role the organization made: what it allows is looked up by the API, nothing until then.
      o3: { role: 'emperor', permissions: {}, groups: [] },
    })
  })
})

describe('bearer authentication', () => {
  let app: App
  beforeAll(async () => {
    app = await testApp()
  })
  afterAll(() => app.close())

  const me = (headers: Record<string, string> = {}) => app.inject({ method: 'GET', url: '/v1/me', headers })
  const bearer = async (...args: Parameters<typeof token>) => ({ authorization: `Bearer ${await token(...args)}` })

  it('requires a token', async () => {
    const res = await me()
    expect(res.statusCode).toBe(401)
    expect(res.headers['content-type']).toMatch(/^application\/problem\+json/)
    expect(res.headers['www-authenticate']).toMatch(/^Bearer/)
    expect(res.json()).toMatchObject({ status: 401, code: 'unauthorized' })
  })

  it('returns the user with their organizations', async () => {
    const res = await me(await bearer({ groups: ['/orgs/o1/roles/admin', '/orgs/o1/groups/dev'] }))
    expect(res.statusCode).toBe(200)
    expect(res.json()).toEqual({
      id: '2c8f1f5e-0000-4000-8000-000000000001',
      email: 'ada@example.com',
      name: 'Ada Lovelace',
      locale: 'de',
      avatar: null,
      roles: ['user', 'offline_access'],
      orgs: [{ id: 'o1', role: 'admin', permissions: defaultPermissions('admin'), groups: ['/orgs/o1/groups/dev'] }],
      activeOrgId: null,
    })
  })

  it('accepts X-Org-Id only for the user\'s orgs', async () => {
    const headers = await bearer({ groups: ['/orgs/o1/roles/member'] })
    const ok = await me({ ...headers, 'x-org-id': 'o1' })
    expect(ok.json().activeOrgId).toBe('o1')
    const other = await me({ ...headers, 'x-org-id': 'o2' })
    expect(other.statusCode).toBe(403)
    expect(other.json().code).toBe('org_forbidden')
  })

  it('rejects expired, foreign and misdirected tokens', async () => {
    const expired = await me(await bearer({}, { expiresIn: Math.floor(Date.now() / 1000) - 60 }))
    expect(expired.statusCode).toBe(401)
    expect(expired.json().detail).toBe('the token has expired')

    const { privateKey } = await generateKeyPair('RS256')
    expect((await me(await bearer({}, { key: privateKey }))).statusCode).toBe(401)

    // Another audience (e.g. a token meant for another service).
    expect((await me(await bearer({ aud: 'account' }))).statusCode).toBe(401)
    // Another realm.
    expect((await me(await bearer({ iss: 'http://localhost:8080/realms/other' }))).statusCode).toBe(401)
    // ID tokens are not access tokens.
    expect((await me(await bearer({ typ: 'ID' }))).statusCode).toBe(401)
    expect((await me({ authorization: 'Bearer not-a-jwt' })).statusCode).toBe(401)
    expect((await me({ authorization: 'Basic abc' })).statusCode).toBe(401)
  })

  it('only accepts tokens of the app\'s own clients', async () => {
    const res = await me(await bearer({ azp: 'some-other-client' }))
    expect(res.statusCode).toBe(403)
    expect(res.json().code).toBe('client_not_allowed')
  })

  it('leaves public routes alone', async () => {
    const res = await app.inject({ method: 'GET', url: '/healthz', headers: { authorization: 'Bearer garbage' } })
    expect(res.statusCode).toBe(200)
  })
})
