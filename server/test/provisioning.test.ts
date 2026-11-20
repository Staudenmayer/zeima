import { and, eq, inArray } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { ensureOrgWorkspace, withOwnerLock } from '../src/modules/orgs/service.ts'
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

  describe('a change made while the organization is being compared with Keycloak', () => {
    it('is not undone by the older list', async () => {
      const org = id(950)
      const u = (n: number) => ({ id: id(n), email: `${n}@race.example`, name: `u${n}`, enabled: true })
      await db.db.insert(schema.organization).values({ id: org, name: 'Race', createdAt: 1 })
      for (const n of [20, 21]) {
        await db.db.insert(schema.kcUser).values({ id: id(n), email: `${n}@race.example`, displayName: `u${n}` }).onConflictDoNothing()
        await db.db.insert(schema.kcOrgMember).values({ orgId: org, userId: id(n), role: n === 20 ? 'owner' : 'member' })
      }
      const racing = new FakeKeycloak()
      racing.members = [u(20), u(21)]
      racing.roles = { owner: [id(20)], admin: [], manager: [], member: [id(21)] }
      // The removal lands while the list is being read: it takes the organization lock like the removal route.
      const list = racing.listMembers.bind(racing)
      let removal: Promise<void> | undefined
      racing.listMembers = async (orgId: string) => {
        const stale = await list(orgId)
        removal ??= withOwnerLock(db.db, org, async (tx) => {
          racing.members = racing.members.filter((m) => m.id !== id(21))
          await tx.delete(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, org), eq(schema.kcOrgMember.userId, id(21))))
        })
        // Done at once if nothing holds the lock; otherwise it waits for the comparison to finish.
        await Promise.race([removal, new Promise((resolve) => setTimeout(resolve, 300))])
        return stale
      }
      await reconcileOrganization(db.db, racing, org)
      await removal
      const members = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, org))
      expect(members.map((m) => m.userId)).toEqual([id(20)])
    })
  })

  describe('many checks of one organization at once', () => {
    it('finish, however small the connection pool is, and share their reads of Keycloak', async () => {
      const org = id(951)
      await db.db.insert(schema.organization).values({ id: org, name: 'Burst', createdAt: 1 })
      await db.db.insert(schema.kcUser).values({ id: id(30), email: '30@burst.example', displayName: 'u30' }).onConflictDoNothing()
      await db.db.insert(schema.kcOrgMember).values({ orgId: org, userId: id(30), role: 'owner' })
      const burst = new FakeKeycloak()
      burst.members = [{ id: id(30), email: '30@burst.example', name: 'u30', enabled: true }, { id: id(31), email: '31@burst.example', name: 'u31', enabled: true }]
      burst.roles = { owner: [id(30)], admin: [], manager: [], member: [id(31)] }
      let reads = 0
      const list = burst.listMembers.bind(burst)
      burst.listMembers = async (orgId: string) => {
        reads++
        await new Promise((resolve) => setTimeout(resolve, 20))
        return list(orgId)
      }
      // Like the licensing lookup, the seat limit needs a connection of its own from the same pool.
      const seatLimit = async () => {
        await db.db.select({ id: schema.organization.id }).from(schema.organization).limit(1)
        return 5
      }
      const all = Promise.all(Array.from({ length: 14 }, () => reconcileOrganization(db.db, burst, org, { seatLimit })))
      const timeout = new Promise<never>((_, reject) => setTimeout(() => reject(new Error('the checks did not finish')), 15_000))
      const results = await Promise.race([all, timeout])
      expect(results.every((r) => r.members === 2)).toBe(true)
      expect(reads).toBeLessThan(14)
      const members = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, org))
      expect(members.map((m) => m.userId).sort()).toEqual([id(30), id(31)])
    }, 30_000)
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


    /** An org workspace and a personal one for `userId`, each with an entry and a pause of that user; the org one also holds `ORG`'s entry of `keeper`. */
    const seedData = async (userId: string, keeper: string) => {
      const n = Math.floor(Math.random() * 1e9)
      const base = { createdAt: 1, updatedAt: 1, hlc: 'h', originDevice: id(800) }
      const orgWs = await ensureOrgWorkspace(db.db, ORG, 'Acme')
      const [personal] = await db.db.insert(schema.workspace).values({ id: id(n % 1e6 + 1000), kind: 'personal', ownerId: userId, name: 'Mine', ...base }).returning()
      const put = async (ws: string, who: string, k: number) => {
        const entry = id(n % 1e6 * 10 + k + 2000000)
        await db.db.insert(schema.timeEntry).values({ id: entry, workspaceId: ws, userId: who, startAt: 1 + k, endAt: 2 + k, tz: 'UTC', source: 'manual', ...base })
        await db.db.insert(schema.pause).values({ id: id(n % 1e6 * 10 + k + 5000000), workspaceId: ws, userId: who, startAt: 1 + k, endAt: 2 + k, kind: 'manual', ...base })
        await db.db.insert(schema.syncRow).values({ workspaceId: ws, tableName: 'time_entry', rowId: entry, fields: { userId: { v: who, h: 'h' } } })
      }
      await put(orgWs, userId, 1)
      await put(orgWs, keeper, 2)
      await put(personal!.id, userId, 3)
      return { orgWs, personalWs: personal!.id }
    }
    const rowsOf = async (ws: string) => ({
      entries: (await db.db.select().from(schema.timeEntry).where(eq(schema.timeEntry.workspaceId, ws))).map((r) => r.userId).sort(),
      pauses: (await db.db.select().from(schema.pause).where(eq(schema.pause.workspaceId, ws))).map((r) => r.userId).sort(),
      sync: (await db.db.select().from(schema.syncRow).where(eq(schema.syncRow.workspaceId, ws))).map((r) => (r.fields as { userId: { v: string } }).userId.v).sort(),
    })

    it('deletes a person\'s data in the organization when SCIM removes them', async () => {
      const person = id(66)
      await db.db.insert(schema.kcUser).values({ id: person, email: 'p66@acme.example', displayName: 'P' }).onConflictDoNothing()
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: person, role: 'member' })
      const { orgWs, personalWs } = await seedData(person, ADMIN)
      expect((await scim('DELETE', `/Users/${person}`)).statusCode).toBe(204)
      const org = await rowsOf(orgWs)
      expect(org.entries).toEqual([ADMIN])
      expect(org.pauses).toEqual([ADMIN])
      expect(org.sync).toEqual([ADMIN])
      expect((await rowsOf(personalWs)).entries).toEqual([person])
      for (const t of [schema.pause, schema.timeEntry, schema.syncRow]) await db.db.delete(t)
      await db.db.delete(schema.workspace)
    })

    /** What stays in the organization's workspace when a person goes: their rows other than entries and pauses. */
    const RETAINED = ['absence', 'work_schedule', 'work_settings', 'balance_adjustment', 'work_location', 'meeting', 'meeting_response']
    const seedRetained = async (ws: string, userId: string, k: number) => {
      for (const [i, table] of RETAINED.entries()) {
        await db.db.insert(schema.syncRow).values({ workspaceId: ws, tableName: table, rowId: id(k * 100 + i + 3000000), fields: { userId: { v: userId, h: 'h' } } })
      }
    }
    const retainedOf = async (ws: string) =>
      (await db.db.select().from(schema.syncRow).where(and(eq(schema.syncRow.workspaceId, ws), inArray(schema.syncRow.tableName, RETAINED)))).map((r) => r.tableName).sort()
    /** With the token the person still holds: pulling returns nothing of the workspace and pushing is refused. */
    const lockedOut = async (userId: string, orgWs: string, headers: { authorization: string }) => {
      const pulled = (await app.inject({ url: '/v1/sync/pull', headers })).json().rows as { workspaceId: string }[]
      expect(pulled.filter((r) => r.workspaceId === orgWs)).toEqual([])
      const pushed = await app.inject({ method: 'POST', url: '/v1/sync/push', headers, payload: { workspaceId: orgWs, deviceId: id(801), changes: [] } })
      expect([403, 404], `push as ${userId}: ${pushed.statusCode}`).toContain(pushed.statusCode)
    }

    it('keeps the organization\'s other records of a person SCIM removes, who loses access to the workspace', async () => {
      const person = id(67)
      await db.db.insert(schema.kcUser).values({ id: person, email: 'p67@acme.example', displayName: 'P' }).onConflictDoNothing()
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: person, role: 'member' })
      const { orgWs } = await seedData(person, ADMIN)
      await seedRetained(orgWs, person, 1)
      const stale = await as(person, 'member')
      expect((await scim('DELETE', `/Users/${person}`)).statusCode).toBe(204)
      expect(await retainedOf(orgWs)).toEqual([...RETAINED].sort())
      await lockedOut(person, orgWs, stale)
      for (const t of [schema.pause, schema.timeEntry, schema.syncRow]) await db.db.delete(t)
      await db.db.delete(schema.workspace)
    })

    it('keeps the organization\'s other records of a person a check against Keycloak finds gone', async () => {
      const person = id(68)
      await db.db.insert(schema.kcUser).values({ id: person, email: 'p68@acme.example', displayName: 'P' }).onConflictDoNothing()
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: person, role: 'member' })
      const { orgWs } = await seedData(person, ADMIN)
      await seedRetained(orgWs, person, 1)
      const stale = await as(person, 'member')
      const members = (await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, ORG))).filter((m) => m.userId !== person)
      kc.members = members.map((m) => ({ id: m.userId, email: `${m.userId.slice(-1)}@example.com`, name: m.userId, enabled: true }))
      kc.roles = { owner: [], admin: [], manager: [], member: [], ...Object.fromEntries(['owner', 'admin', 'manager', 'member'].map((r) => [r, members.filter((m) => m.role === r).map((m) => m.userId)])) }
      await reconcileOrganization(db.db, kc, ORG)
      expect((await db.db.select().from(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, ORG), eq(schema.kcOrgMember.userId, person)))).length).toBe(0)
      expect(await retainedOf(orgWs)).toEqual([...RETAINED].sort())
      expect((await rowsOf(orgWs)).entries).toEqual([ADMIN])
      await lockedOut(person, orgWs, stale)
      for (const t of [schema.pause, schema.timeEntry, schema.syncRow]) await db.db.delete(t)
      await db.db.delete(schema.workspace)
    })

    it('never changes an owner, or someone outside the domains', async () => {
      const owner = await scim('PATCH', `/Users/${OWNER}`, { Operations: [{ op: 'replace', path: 'active', value: false }] })
      expect(owner.statusCode).toBe(403)
      const outside = await scim('DELETE', `/Users/${MEMBER}`)
      expect(outside.statusCode).toBe(403)
    })

    it('never changes an administrator, even inside the domains (a key is not bound by the role hierarchy)', async () => {
      await db.db.update(schema.kcUser).set({ email: 'adam@acme.example' }).where(eq(schema.kcUser.id, ADMIN))
      const off = await scim('PATCH', `/Users/${ADMIN}`, { Operations: [{ op: 'replace', path: 'active', value: false }] })
      const gone = await scim('DELETE', `/Users/${ADMIN}`)
      await db.db.update(schema.kcUser).set({ email: '2@example.com' }).where(eq(schema.kcUser.id, ADMIN))
      expect(off.statusCode).toBe(403)
      expect(gone.statusCode).toBe(403)
      expect((await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, ADMIN))).length).toBe(1)
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
