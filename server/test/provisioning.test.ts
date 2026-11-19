import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { reconcileOrganization } from '../src/modules/orgs/reconcile.ts'
import { changesOf, emailFromFilter, hashToken } from '../src/modules/orgs/scim.ts'
import { checkFilter, checkLdapUrl } from '../src/modules/directory/routes.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { keys, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const OWNER = id(1), ADMIN = id(2), MEMBER = id(4)
const ORG = id(900)

describe('SCIM and directory input', () => {
  it('reads the filters identity providers send', () => {
    expect(emailFromFilter(undefined)).toBeUndefined()
    expect(emailFromFilter('userName eq "Ann@Acme.example"')).toBe('ann@acme.example')
    expect(emailFromFilter('emails.value eq "a@b.co"')).toBe('a@b.co')
    expect(() => emailFromFilter('userName co "x"')).toThrow()
  })

  it('reads what a PATCH changes, however the provider spells it', () => {
    expect(changesOf({ Operations: [{ op: 'Replace', path: 'active', value: 'False' }] }, true)).toEqual({ active: false })
    expect(changesOf({ Operations: [{ op: 'replace', value: { active: true, name: { givenName: 'Ann' } } }] }, true)).toEqual({ active: true, givenName: 'Ann' })
    expect(changesOf({ Operations: [{ op: 'replace', path: 'name.familyName', value: 'Lee' }, { op: 'remove', path: 'active' }] }, true)).toEqual({ familyName: 'Lee' })
    expect(changesOf({ active: false, name: { givenName: 'A', familyName: 'B' } }, false)).toEqual({ active: false, givenName: 'A', familyName: 'B' })
    expect(() => changesOf({}, true)).toThrow()
  })

  it('checks the directory address and filter', () => {
    expect(() => checkLdapUrl('ldaps://ad.acme.example:636')).not.toThrow()
    expect(() => checkLdapUrl('ldap://ad.acme.example:389')).toThrow()
    expect(() => checkLdapUrl('ldap://ad.acme.example:389', true)).not.toThrow()
    for (const bad of ['http://x', 'ldap://u:p@x', 'nonsense', 'ldap://x/dc=a']) expect(() => checkLdapUrl(bad), bad).toThrow()
    expect(() => checkFilter(undefined)).not.toThrow()
    expect(() => checkFilter('(memberOf=cn=zeima,dc=a)')).not.toThrow()
    for (const bad of ['memberOf=x', '(a=b', '(a=b))(']) expect(() => checkFilter(bad), bad).toThrow()
  })
})

describe.skipIf(!(await reachable()))('provisioning', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  let kc: FakeKeycloak
  let app: Awaited<ReturnType<typeof buildApp>>

  const as = async (userId: string, role: string | null, extra: object = {}) => ({
    authorization: `Bearer ${await token({ sub: userId, email: `${userId.slice(-1)}@example.com`, groups: role ? [`/orgs/${ORG}/roles/${role}`] : [], ...extra })}`,
  })

  beforeAll(async () => {
    db = await createTestDatabase()
    kc = new FakeKeycloak()
    app = await buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false' }), { logger: false, keys, readiness: {}, db: db.db, keycloakAdmin: kc, documentAll: true })
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    for (const [uid, role] of [[OWNER, 'owner'], [ADMIN, 'admin'], [MEMBER, 'member']] as const) {
      await db.db.insert(schema.kcUser).values({ id: uid, email: `${uid.slice(-1)}@example.com`, displayName: role }).onConflictDoNothing()
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: uid, role })
    }
  })
  afterAll(async () => {
    await app?.close()
    await db?.drop()
  })

  describe('seats for people who join through single sign-on', () => {
    const kcUser = (n: number, email: string) => ({ id: id(n), email, name: email, enabled: true })

    it('lets newcomers in while seats are free and turns the rest away', async () => {
      await db.db.insert(schema.entitlement).values({ id: id(700), orgId: ORG, plan: 'enterprise', seats: 5, updatedAt: 1 })
      const seatLimit = async () => 5
      kc.members = [kcUser(1, '1@example.com'), kcUser(2, '2@example.com'), kcUser(4, '4@example.com'), kcUser(10, 'a@acme.example'), kcUser(11, 'b@acme.example'), kcUser(12, 'c@acme.example')]
      kc.roles = { owner: [id(1)], admin: [id(2)], manager: [], member: [id(4)] }
      kc.calls = []
      const result = await reconcileOrganization(db.db, kc, ORG, { seatLimit })
      expect(result.rejectedMembers).toBe(1)
      expect(kc.calls.filter((c) => c.startsWith('removeMember'))).toEqual([`removeMember ${ORG} ${id(12)}`])
      const members = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, ORG))
      expect(members).toHaveLength(5)
      const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.orgId, ORG))
      expect(log.find((l) => l.action === 'member.rejected')).toMatchObject({ target: id(12), detail: { reason: 'seat_limit', limit: 5 } })
    })

    it('does not turn away someone who has an invitation, and does not remove people who are already in', async () => {
      await db.db.insert(schema.orgInvitation).values({ orgId: ORG, email: 'd@acme.example', role: 'member', createdAt: 1 })
      // 5 members + 1 pending invitation already exceeds 5 seats: the invited person still comes in, others do not.
      kc.members = [...kc.members.filter((m) => m.id !== id(12)), kcUser(13, 'd@acme.example'), kcUser(14, 'e@acme.example')]
      const result = await reconcileOrganization(db.db, kc, ORG, { seatLimit: async () => 5 })
      expect(result.rejectedMembers).toBe(1)
      const members = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, ORG))
      expect(members.map((m) => m.userId)).toContain(id(13))
      expect(members.map((m) => m.userId)).not.toContain(id(14))
      expect(members).toHaveLength(6)
    })

    it('has no limit when there is none', async () => {
      kc.members = [...kc.members.filter((m) => m.id !== id(14)), kcUser(15, 'f@acme.example')]
      const result = await reconcileOrganization(db.db, kc, ORG, { seatLimit: async () => null })
      expect(result.rejectedMembers).toBe(0)
      expect((await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, ORG))).length).toBe(7)
    })
  })

  describe('a disabled account', () => {
    it('is signed out when Keycloak says it was disabled, and refused at once', async () => {
      kc.members = kc.members.map((m) => (m.id === id(10) ? { ...m, enabled: false } : m))
      kc.calls = []
      const result = await reconcileOrganization(db.db, kc, ORG)
      expect(result.signedOut).toBe(1)
      expect(kc.calls).toContain(`logoutUser ${id(10)}`)
      const res = await app.inject({ method: 'GET', url: '/v1/me', headers: await as(id(10), 'member') })
      expect(res.statusCode).toBe(401)
      expect(res.json().detail).toMatch(/disabled/)
    })

    it('is signed out only once', async () => {
      kc.calls = []
      const result = await reconcileOrganization(db.db, kc, ORG)
      expect(result.signedOut).toBe(0)
      expect(kc.calls.filter((c) => c.startsWith('logoutUser'))).toEqual([])
    })
  })

  describe('SCIM', () => {
    let key = ''
    beforeAll(async () => {
      await db.db.update(schema.entitlement).set({ seats: 50 })
    })
    const scim = async (method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE', path: string, payload?: object, auth = key) =>
      app.inject({ method, url: `/v1/scim/v2${path}`, headers: { authorization: `Bearer ${auth}`, ...(payload && { 'content-type': 'application/scim+json' }) }, payload: payload && JSON.stringify(payload) })
    const admin = async (method: 'GET' | 'POST' | 'DELETE', path: string, userId = ADMIN, role: string | null = 'admin', payload?: object) =>
      app.inject({ method, url: `/v1/orgs/${ORG}/scim${path}`, headers: await as(userId, role), payload })

    it('makes keys for owners and admins only; the key is shown once', async () => {
      expect((await admin('POST', '/tokens', MEMBER, 'member', { name: 'x' })).statusCode).toBe(403)
      const res = await admin('POST', '/tokens', ADMIN, 'admin', { name: 'Entra' })
      expect(res.statusCode).toBe(201)
      key = res.json().token
      expect(key).toMatch(/^zscim_/)
      const list = (await admin('GET', '')).json()
      expect(list.baseUrl).toBe('http://localhost:3000/v1/scim/v2')
      expect(list.tokens).toHaveLength(1)
      expect(JSON.stringify(list)).not.toContain(key)
      const rows = await db.db.select().from(schema.scimToken)
      expect(rows[0]!.hash).toBe(hashToken(key))
      expect(rows[0]!.hash).not.toContain(key)
    })

    it('refuses a missing or wrong key', async () => {
      expect((await scim('GET', '/Users', undefined, 'zscim_nope')).statusCode).toBe(401)
      const res = await app.inject({ method: 'GET', url: '/v1/scim/v2/Users' })
      expect(res.statusCode).toBe(401)
      expect(res.json().schemas).toEqual(['urn:ietf:params:scim:api:messages:2.0:Error'])
    })

    it('lists and finds people', async () => {
      const list = (await scim('GET', '/Users')).json()
      expect(list.totalResults).toBeGreaterThanOrEqual(3)
      const one = (await scim('GET', `/Users?filter=${encodeURIComponent('userName eq "4@example.com"')}`)).json()
      expect(one.Resources.map((r: { id: string }) => r.id)).toEqual([MEMBER])
      expect((await scim('GET', '/Users?filter=active%20eq%20true')).statusCode).toBe(400)
      expect((await scim('GET', '/ServiceProviderConfig')).json().patch.supported).toBe(true)
    })

    it('only provisions people in a domain the organization set up for sign-in', async () => {
      const body = { schemas: ['urn:ietf:params:scim:schemas:core:2.0:User'], userName: 'new@acme.example', name: { givenName: 'Nia', familyName: 'Neu' }, active: true }
      expect((await scim('POST', '/Users', body)).statusCode).toBe(400)
      kc.sso.set(ORG, { protocol: 'oidc', name: 'Acme', enabled: true, sourceUrl: 'https://login.acme.example', clientId: 'z', domains: ['acme.example'] })
      const res = await scim('POST', '/Users', body)
      expect(res.statusCode).toBe(201)
      expect(res.headers.location).toContain('/v1/scim/v2/Users/')
      expect(res.json()).toMatchObject({ userName: 'new@acme.example', displayName: 'Nia Neu', active: true })
      const created = res.json().id
      expect(kc.calls).toContain(`addMember ${ORG} ${created}`)
      expect((await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, created)))[0]!.role).toBe('member')
      // Twice is a conflict.
      expect((await scim('POST', '/Users', body)).statusCode).toBe(409)
      // Outside the domains: refused.
      expect((await scim('POST', '/Users', { ...body, userName: 'x@other.example' })).statusCode).toBe(400)
    })

    it('disables, re-enables, renames and removes', async () => {
      const found = (await scim('GET', `/Users?filter=${encodeURIComponent('userName eq "new@acme.example"')}`)).json().Resources[0]
      kc.calls = []
      const off = await scim('PATCH', `/Users/${found.id}`, { schemas: ['urn:ietf:params:scim:api:messages:2.0:PatchOp'], Operations: [{ op: 'Replace', path: 'active', value: 'False' }] })
      expect(off.statusCode).toBe(200)
      expect(off.json().active).toBe(false)
      expect(kc.calls).toEqual([`setEnabled ${found.id} false`, `logoutUser ${found.id}`])
      const on = await scim('PUT', `/Users/${found.id}`, { userName: 'new@acme.example', active: true, name: { givenName: 'Nia', familyName: 'Neumann' } })
      expect(on.json()).toMatchObject({ active: true, displayName: 'Nia Neumann' })
      const log = (await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.orgId, ORG))).map((l) => l.action)
      expect(log).toEqual(expect.arrayContaining(['member.deactivate', 'member.reactivate']))
      expect((await scim('DELETE', `/Users/${found.id}`)).statusCode).toBe(204)
      expect((await scim('GET', `/Users/${found.id}`)).statusCode).toBe(404)
      expect(kc.calls).toContain(`removeMember ${ORG} ${found.id}`)
    })

    it('never changes an owner, or someone outside the domains', async () => {
      const owner = await scim('PATCH', `/Users/${OWNER}`, { Operations: [{ op: 'replace', path: 'active', value: false }] })
      expect(owner.statusCode).toBe(403)
      const outside = await scim('DELETE', `/Users/${MEMBER}`)
      expect(outside.statusCode).toBe(403)
    })

    it('does not pull an existing account into the organization: the person is invited', async () => {
      const outsider = id(60)
      kc.users.set(outsider, { id: outsider, email: 'old@acme.example', name: 'Old Account', enabled: true })
      kc.calls = []
      const res = await scim('POST', '/Users', { userName: 'old@acme.example', name: { givenName: 'Old' } })
      expect(res.statusCode).toBe(409)
      // The answer is the same as for a person who is already a member: it does not say that an account exists elsewhere.
      kc.users.set(id(10), { id: id(10), email: 'a@acme.example', name: 'A', enabled: true })
      const already = await scim('POST', '/Users', { userName: 'a@acme.example', name: { givenName: 'M' } })
      expect(already.statusCode).toBe(409)
      expect(res.json().detail).toBe(already.json().detail)
      expect(res.json().scimType).toBe(already.json().scimType)
      expect(kc.calls).toEqual([`invite ${ORG} old@acme.example`])
      expect(kc.calls.join()).not.toContain('addMember')
      expect((await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, outsider))).length).toBe(0)
      expect((await db.db.select().from(schema.orgInvitation).where(eq(schema.orgInvitation.email, 'old@acme.example'))).length).toBe(1)
    })

    it('marks an address verified only when the domain was proven', async () => {
      kc.calls = []
      expect((await scim('POST', '/Users', { userName: 'proven@acme.example', name: { givenName: 'P' } })).statusCode).toBe(201)
      expect(kc.calls).toContain('createUser proven@acme.example verified')
      const lax = await buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false', SSO_VERIFY_DOMAINS: 'false' }), { logger: false, keys, readiness: {}, db: db.db, keycloakAdmin: kc, documentAll: true })
      try {
        kc.calls = []
        const res = await lax.inject({ method: 'POST', url: '/v1/scim/v2/Users', headers: { authorization: `Bearer ${key}`, 'content-type': 'application/scim+json' }, payload: JSON.stringify({ userName: 'unproven@acme.example', name: { givenName: 'U' } }) })
        expect(res.statusCode).toBe(201)
        expect(kc.calls).toContain('createUser unproven@acme.example unverified')
      } finally {
        await lax.close()
      }
    })

    it('does not switch off an account that belongs to another organization: it only takes the person out of this one', async () => {
      const shared = (await scim('GET', `/Users?filter=${encodeURIComponent('userName eq "proven@acme.example"')}`)).json().Resources[0]
      const other = id(901)
      await db.db.insert(schema.organization).values({ id: other, name: 'Other', createdAt: 1 })
      await db.db.insert(schema.kcOrgMember).values({ orgId: other, userId: shared.id, role: 'member' })
      kc.calls = []
      const res = await scim('PATCH', `/Users/${shared.id}`, { Operations: [{ op: 'Replace', path: 'active', value: false }] })
      expect(res.statusCode).toBe(200)
      expect(res.json().active).toBe(false)
      expect(kc.calls).toEqual([`removeMember ${ORG} ${shared.id}`])
      expect(kc.calls.join()).not.toContain('setEnabled')
      const stillThere = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, shared.id))
      expect(stillThere.map((m) => m.orgId)).toEqual([other])
      expect((await db.db.select().from(schema.kcUser).where(eq(schema.kcUser.id, shared.id)))[0]!.active).toBe(true)
    })

    it('stops at the seat limit', async () => {
      await db.db.update(schema.entitlement).set({ seats: 1 })
      const res = await scim('POST', '/Users', { userName: 'late@acme.example', name: { givenName: 'L' } })
      expect(res.statusCode).toBe(409)
      expect(res.json().detail).toMatch(/seats/)
    })

    it('stops working when the key is removed', async () => {
      const list = (await admin('GET', '')).json()
      expect((await admin('DELETE', `/tokens/${list.tokens[0].id}`)).statusCode).toBe(204)
      expect((await scim('GET', '/Users')).statusCode).toBe(401)
    })
  })

  describe('the directory (LDAP / Active Directory)', () => {
    const PLATFORM = { sub: id(3), roles: ['platform-admin'] }
    const dir = async (method: 'GET' | 'PUT' | 'POST' | 'DELETE', path: string, payload?: object, admin = true) =>
      app.inject({
        method,
        url: `/v1/admin/ldap${path}`,
        headers: { authorization: `Bearer ${await token(admin ? { sub: PLATFORM.sub, realm_access: { roles: PLATFORM.roles } } : { sub: id(4) })}` },
        payload,
      })
    const LDAP = { vendor: 'ad', url: 'ldaps://ad.acme.example:636', bindDn: 'CN=svc,OU=Service,DC=acme,DC=example', bindPassword: 'pw', usersDn: 'OU=People,DC=acme,DC=example' }

    it('is for the people who run the installation', async () => {
      expect((await dir('GET', '', undefined, false)).statusCode).toBe(403)
      expect((await dir('PUT', '', LDAP, false)).statusCode).toBe(403)
      expect((await dir('GET', '')).json()).toMatchObject({ configured: false })
    })

    it('checks the connection, saves, and never gives the password back', async () => {
      kc.calls = []
      const res = await dir('PUT', '', LDAP)
      expect(res.statusCode).toBe(200)
      expect(res.json()).toMatchObject({ configured: true, vendor: 'ad', url: LDAP.url, usersDn: LDAP.usersDn, enabled: true })
      expect(JSON.stringify(res.json())).not.toContain('pw"')
      expect(kc.calls).toEqual([`testLdap ${LDAP.url}`, `setLdap ad ${LDAP.url}`])
    })

    it('keeps the password on a change, and needs one the first time', async () => {
      const { bindPassword: _p, ...without } = LDAP
      expect((await dir('PUT', '', { ...without, usersDn: 'OU=Staff,DC=acme,DC=example' })).statusCode).toBe(200)
      expect(kc.ldap?.bindPassword).toBe('pw')
      await dir('DELETE', '')
      expect(kc.ldap).toBeNull()
      expect((await dir('PUT', '', without)).statusCode).toBe(400)
      expect((await dir('PUT', '', { ...LDAP, url: 'http://x' })).statusCode).toBe(400)
    })

    it('does not save what Keycloak cannot connect to', async () => {
      kc.failNext = 'testLdap'
      const res = await dir('PUT', '', LDAP)
      expect(res.statusCode).toBe(500)
      expect(kc.ldap).toBeNull()
    })

    it('explains what to do when the service account has no right to change the realm (the default)', async () => {
      kc.forbidLdap = true
      try {
        const res = await dir('GET', '')
        expect(res.statusCode).toBe(409)
        expect(res.json().code).toBe('directory_needs_role')
        expect(res.json().detail).toContain('manage-realm')
      } finally {
        kc.forbidLdap = false
      }
    })

    it('imports people on request', async () => {
      await dir('PUT', '', LDAP)
      const res = await dir('POST', '/sync')
      expect(res.json()).toEqual({ added: 2, updated: 1, removed: 0, failed: 0 })
    })
  })
})

describe.skipIf(!(await reachable()))('an organization Keycloak announces before the API has stored it', () => {
  it('is skipped, not an error', async () => {
    const db = await createTestDatabase()
    try {
      const kc = new FakeKeycloak()
      kc.members = [{ id: id(1), email: 'a@example.com', name: 'A', enabled: true }]
      const result = await reconcileOrganization(db.db, kc, id(777))
      expect(result).toMatchObject({ members: 0, addedMembers: 0 })
      expect((await db.db.select().from(schema.kcOrgMember)).length).toBe(0)
    } finally {
      await db.drop()
    }
  })
})
