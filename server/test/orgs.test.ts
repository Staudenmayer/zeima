import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { keys, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const OWNER = id(1), ADMIN = id(2), MANAGER = id(3), MEMBER = id(4), OTHER = id(5)
const ORG = id(900)
const NAMES: Record<string, string> = { [OWNER]: 'Olga', [ADMIN]: 'Adam', [MANAGER]: 'Mia', [MEMBER]: 'Max', [OTHER]: 'Otto' }

describe.skipIf(!(await reachable()))('organizations', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  let kc: FakeKeycloak
  let app: Awaited<ReturnType<typeof build>>

  const build = () =>
    buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false' }), {
      logger: false,
      keys,
      readiness: {},
      db: db.db,
      keycloakAdmin: kc,
    })

  /** Calls the API as `userId` with `role` in the test organization. */
  const as = async (userId: string, role: string | null, extra: { email?: string; verified?: boolean; groups?: string[] } = {}) => ({
    authorization: `Bearer ${await token({
      sub: userId,
      name: NAMES[userId] ?? 'Someone',
      email: extra.email ?? `${userId.slice(-1)}@example.com`,
      email_verified: extra.verified ?? true,
      groups: [...(role ? [`/orgs/${ORG}/roles/${role}`] : []), ...(extra.groups ?? [])],
    })}`,
  })
  const call = async (method: 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE', url: string, userId: string, role: string | null, payload?: object) =>
    app.inject({ method, url, headers: await as(userId, role), payload })

  beforeAll(async () => {
    db = await createTestDatabase()
    kc = new FakeKeycloak()
    app = await build()
  })
  afterAll(async () => {
    await app?.close()
    await db?.drop()
  })

  const reset = async () => {
    kc.calls = []
    await db.db.delete(schema.workspace).where(eq(schema.workspace.kind, 'org'))
    await db.db.delete(schema.organization)
    await db.db.delete(schema.orgInvitation)
    await db.db.delete(schema.entitlement)
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    for (const [uid, name, role] of [
      [OWNER, 'Olga', 'owner'],
      [ADMIN, 'Adam', 'admin'],
      [MANAGER, 'Mia', 'manager'],
      [MEMBER, 'Max', 'member'],
    ] as const) {
      await db.db
        .insert(schema.kcUser)
        .values({ id: uid, email: `${uid.slice(-1)}@example.com`, displayName: name })
        .onConflictDoUpdate({ target: schema.kcUser.id, set: { displayName: name, active: true } })
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: uid, role })
    }
    await db.db.insert(schema.kcUser).values({ id: OTHER, email: 'other@example.com', displayName: 'Otto' }).onConflictDoNothing()
  }

  it('creates an organization and makes the creator its owner', async () => {
    await reset()
    const res = await app.inject({ method: 'POST', url: '/v1/orgs', headers: await as(OTHER, null), payload: { name: ' Globex ' } })
    expect(res.statusCode).toBe(201)
    const { id: orgId, name } = res.json()
    expect(name).toBe('Globex')
    expect(kc.calls).toEqual([`createOrganization Globex`, `addMember ${orgId} ${OTHER}`, `setRole ${orgId} ${OTHER} owner`])
    const [member] = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, orgId))
    expect(member).toMatchObject({ userId: OTHER, role: 'owner' })
    const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.orgId, orgId))
    expect(log.map((l) => l.action)).toEqual(['org.create'])
  })

  it('removes the Keycloak organization again if the database step fails', async () => {
    await reset()
    kc.failNext = 'setRole'
    const res = await app.inject({ method: 'POST', url: '/v1/orgs', headers: await as(OTHER, null), payload: { name: 'Initech' } })
    expect(res.statusCode).toBe(500)
    expect(kc.calls.at(-1)).toMatch(/^deleteOrganization /)
  })

  it('is off without Keycloak admin credentials', async () => {
    const plain = await buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false' }), {
      logger: false,
      keys,
      readiness: {},
      db: db.db,
    })
    const res = await plain.inject({ method: 'POST', url: '/v1/orgs', headers: await as(OTHER, null), payload: { name: 'X' } })
    expect(res.statusCode).toBe(503)
    expect(res.json().code).toBe('org_admin_unavailable')
    await plain.close()
  })

  it('shows the organization with seat usage; strangers are refused', async () => {
    await reset()
    await db.db.insert(schema.entitlement).values({ id: id(700), orgId: ORG, plan: 'team', seats: 10, updatedAt: 1 })
    const res = await call('GET', `/v1/orgs/${ORG}`, MEMBER, 'member')
    expect(res.statusCode).toBe(200)
    expect(res.json()).toMatchObject({ id: ORG, name: 'Acme', role: 'member', seats: { limit: 10, members: 4, pending: 0 } })
    expect((await call('GET', `/v1/orgs/${ORG}`, OTHER, null)).statusCode).toBe(403)
    expect((await app.inject(`/v1/orgs/${ORG}`)).statusCode).toBe(401)
  })

  it('lists members by what the role may see', async () => {
    await reset()
    const names = async (uid: string, role: string, groups: string[] = []) => {
      const res = await app.inject({ url: `/v1/orgs/${ORG}/members`, headers: await as(uid, role, { groups }) })
      expect(res.statusCode).toBe(200)
      return res.json().map((m: { name: string }) => m.name)
    }
    expect(await names(OWNER, 'owner')).toEqual(['Adam', 'Max', 'Mia', 'Olga'])
    expect(await names(ADMIN, 'admin')).toEqual(['Adam', 'Max', 'Mia', 'Olga'])
    expect(await names(MEMBER, 'member')).toEqual(['Max'])
    // A manager sees themselves and the people in their own groups.
    const group = `/orgs/${ORG}/groups/dev`
    const [g] = await db.db.insert(schema.kcGroup).values({ id: id(800), orgId: ORG, path: group }).returning()
    await db.db.insert(schema.kcGroupMember).values([
      { groupId: g!.id, userId: MANAGER },
      { groupId: g!.id, userId: MEMBER },
    ])
    expect(await names(MANAGER, 'manager', [group])).toEqual(['Max', 'Mia'])
    expect(await names(MANAGER, 'manager')).toEqual(['Mia'])
  })

  describe('invitations', () => {
    it('invites with a role; only owners may invite admins and managers', async () => {
      await reset()
      const ok = await call('POST', `/v1/orgs/${ORG}/invitations`, ADMIN, 'admin', { email: 'New@Example.com' })
      expect(ok.statusCode).toBe(201)
      expect(ok.json()).toMatchObject({ email: 'new@example.com', role: 'member' })
      expect(kc.calls).toEqual([`invite ${ORG} new@example.com`])
      expect((await call('POST', `/v1/orgs/${ORG}/invitations`, ADMIN, 'admin', { email: 'boss@example.com', role: 'admin' })).statusCode).toBe(403)
      expect((await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'boss@example.com', role: 'admin' })).statusCode).toBe(201)
      expect((await call('POST', `/v1/orgs/${ORG}/invitations`, MEMBER, 'member', { email: 'x@example.com' })).statusCode).toBe(403)
      const list = await call('GET', `/v1/orgs/${ORG}/invitations`, ADMIN, 'admin')
      expect(list.json().map((i: { email: string }) => i.email).sort()).toEqual(['boss@example.com', 'new@example.com'])
    })

    it('refuses duplicates, members and an invalid address', async () => {
      await reset()
      const post = (email: string) => call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email })
      expect((await post('twice@example.com')).statusCode).toBe(201)
      const again = await post('TWICE@example.com')
      expect(again.statusCode).toBe(409)
      expect(again.json().code).toBe('already_invited')
      const member = await post(`${MEMBER.slice(-1)}@example.com`)
      expect(member.json().code).toBe('already_member')
      expect((await post('not-an-address')).statusCode).toBe(400)
    })

    it('counts pending invitations against the seats', async () => {
      await reset()
      await db.db.insert(schema.entitlement).values({ id: id(700), orgId: ORG, plan: 'team', seats: 5, updatedAt: 1 })
      expect((await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'one@example.com' })).statusCode).toBe(201)
      const full = await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'two@example.com' })
      expect(full.statusCode).toBe(409)
      expect(full.json().code).toBe('seat_limit_reached')
      // Withdrawing frees the seat.
      const [inv] = (await call('GET', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner')).json()
      expect((await call('DELETE', `/v1/orgs/${ORG}/invitations/${inv.id}`, OWNER, 'owner')).statusCode).toBe(204)
      expect((await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'two@example.com' })).statusCode).toBe(201)
    })

    it('forgets the invitation when Keycloak cannot send it', async () => {
      await reset()
      kc.failNext = 'invite'
      expect((await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'late@example.com' })).statusCode).toBe(500)
      expect(await db.db.select().from(schema.orgInvitation)).toEqual([])
    })

    it('gives the role to the invitee on sign-in, if their email is verified', async () => {
      await reset()
      await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'ines@example.com', role: 'manager' })
      kc.calls = []
      const NEW = id(60)
      // Unverified address: nothing happens.
      await app.inject({ url: '/v1/me', headers: await as(NEW, null, { email: 'ines@example.com', verified: false }) })
      expect(kc.calls).toEqual([])
      expect(await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, NEW))).toEqual([])
    })

    it('applies the invitation once the address is verified', async () => {
      await reset()
      await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'ines2@example.com', role: 'manager' })
      kc.calls = []
      const NEW = id(61)
      expect((await app.inject({ url: '/v1/me', headers: await as(NEW, null, { email: 'Ines2@example.com' }) })).statusCode).toBe(200)
      expect(kc.calls).toEqual([`addMember ${ORG} ${NEW}`, `setRole ${ORG} ${NEW} manager`])
      expect(await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, NEW))).toMatchObject([{ role: 'manager' }])
      const [inv] = await db.db.select().from(schema.orgInvitation)
      expect(inv!.acceptedAt).not.toBeNull()
      const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.action, 'member.join'))
      expect(log).toHaveLength(1)
    })
  })

  describe('changing members', () => {
    it('lets only owners change roles, and keeps one owner', async () => {
      await reset()
      const patch = (uid: string, role: string, target: string, body: object) => call('PATCH', `/v1/orgs/${ORG}/members/${target}`, uid, role, body)
      expect((await patch(ADMIN, 'admin', MEMBER, { role: 'manager' })).statusCode).toBe(403)
      const ok = await patch(OWNER, 'owner', MEMBER, { role: 'manager' })
      expect(ok.statusCode).toBe(200)
      expect(ok.json().role).toBe('manager')
      expect(kc.calls).toEqual([`setRole ${ORG} ${MEMBER} manager`])
      const last = await patch(OWNER, 'owner', OWNER, { role: 'admin' })
      expect(last.statusCode).toBe(409)
      expect(last.json().code).toBe('last_owner')
      // With a second owner the first may step down.
      await patch(OWNER, 'owner', ADMIN, { role: 'owner' })
      expect((await patch(OWNER, 'owner', OWNER, { role: 'admin' })).statusCode).toBe(200)
      const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.action, 'member.role'))
      expect(log).toHaveLength(3)
    })

    it('deactivates and reactivates accounts', async () => {
      await reset()
      const patch = (uid: string, role: string, target: string, body: object) => call('PATCH', `/v1/orgs/${ORG}/members/${target}`, uid, role, body)
      expect((await patch(MEMBER, 'member', MANAGER, { active: false })).statusCode).toBe(403)
      expect((await patch(ADMIN, 'admin', OWNER, { active: false })).statusCode).toBe(403)
      expect((await patch(ADMIN, 'admin', ADMIN, { active: false })).json().code).toBe('self')
      const off = await patch(ADMIN, 'admin', MEMBER, { active: false })
      expect(off.json().active).toBe(false)
      expect(kc.calls).toEqual([`setEnabled ${MEMBER} false`])
      expect((await patch(ADMIN, 'admin', MEMBER, { active: true })).json().active).toBe(true)
      // Someone in two organizations is removed from this one instead.
      const ORG2 = id(901)
      await db.db.insert(schema.organization).values({ id: ORG2, name: 'Other', createdAt: 1 })
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG2, userId: MEMBER, role: 'member' })
      expect((await patch(ADMIN, 'admin', MEMBER, { active: false })).json().code).toBe('member_in_other_orgs')
    })

    it('needs something to change and a member to change', async () => {
      await reset()
      expect((await call('PATCH', `/v1/orgs/${ORG}/members/${MEMBER}`, OWNER, 'owner', {})).statusCode).toBe(400)
      expect((await call('PATCH', `/v1/orgs/${ORG}/members/${OTHER}`, OWNER, 'owner', { role: 'admin' })).statusCode).toBe(404)
    })

    it('removes members; people can leave; the last owner cannot', async () => {
      await reset()
      expect((await call('DELETE', `/v1/orgs/${ORG}/members/${MANAGER}`, MEMBER, 'member')).statusCode).toBe(403)
      expect((await call('DELETE', `/v1/orgs/${ORG}/members/${OWNER}`, ADMIN, 'admin')).statusCode).toBe(403)
      expect((await call('DELETE', `/v1/orgs/${ORG}/members/${MANAGER}`, ADMIN, 'admin')).statusCode).toBe(204)
      expect(kc.calls).toEqual([`removeMember ${ORG} ${MANAGER}`])
      // Leaving needs no special right.
      expect((await call('DELETE', `/v1/orgs/${ORG}/members/${MEMBER}`, MEMBER, 'member')).statusCode).toBe(204)
      const last = await call('DELETE', `/v1/orgs/${ORG}/members/${OWNER}`, OWNER, 'owner')
      expect(last.statusCode).toBe(409)
      expect(last.json().code).toBe('last_owner')
      const left = await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, ORG))
      expect(left.map((m) => m.userId).sort()).toEqual([OWNER, ADMIN])
    })
  })

  describe('departments', () => {
    it('creates nested departments, fills them and deletes them', async () => {
      await reset()
      const post = (uid: string, role: string, body: object) => call('POST', `/v1/orgs/${ORG}/groups`, uid, role, body)
      expect((await post(MANAGER, 'manager', { name: 'dev' })).statusCode).toBe(403)
      const dev = await post(ADMIN, 'admin', { name: 'dev' })
      expect(dev.statusCode).toBe(201)
      expect(dev.json()).toMatchObject({ name: 'dev', path: 'dev', parentId: null, memberIds: [] })
      const backend = await post(ADMIN, 'admin', { name: 'backend', parentId: dev.json().id })
      expect(backend.json()).toMatchObject({ path: 'dev/backend', parentId: dev.json().id })
      expect(kc.calls).toEqual([`createGroup ${ORG} dev`, `createGroup ${ORG} dev/backend`])
      const clash = await post(ADMIN, 'admin', { name: 'dev' })
      expect(clash.statusCode).toBe(409)
      expect((await post(ADMIN, 'admin', { name: 'a/b' })).statusCode).toBe(400)
      expect((await post(ADMIN, 'admin', { name: 'x', parentId: id(999) })).statusCode).toBe(404)

      const gid = backend.json().id
      const put = (userId: string) => call('PUT', `/v1/orgs/${ORG}/groups/${gid}/members/${userId}`, ADMIN, 'admin')
      expect((await put(MEMBER)).statusCode).toBe(204)
      expect((await put(MEMBER)).statusCode).toBe(204)
      expect((await put(OTHER)).statusCode).toBe(404)
      const list = await call('GET', `/v1/orgs/${ORG}/groups`, ADMIN, 'admin')
      expect(list.json().map((g: { path: string; memberIds: string[] }) => [g.path, g.memberIds])).toEqual([
        ['dev', []],
        ['dev/backend', [MEMBER]],
      ])
      // Others see only their own departments.
      const mine = await app.inject({ url: `/v1/orgs/${ORG}/groups`, headers: await as(MEMBER, 'member', { groups: [`/orgs/${ORG}/groups/dev/backend`] }) })
      expect(mine.json().map((g: { path: string }) => g.path)).toEqual(['dev/backend'])
      expect((await call('GET', `/v1/orgs/${ORG}/groups`, MEMBER, 'member')).json()).toEqual([])

      expect((await call('DELETE', `/v1/orgs/${ORG}/groups/${gid}/members/${MEMBER}`, ADMIN, 'admin')).statusCode).toBe(204)
      expect(await db.db.select().from(schema.kcGroupMember)).toEqual([])
      // Deleting a department takes the ones below it along.
      expect((await call('DELETE', `/v1/orgs/${ORG}/groups/${dev.json().id}`, ADMIN, 'admin')).statusCode).toBe(204)
      expect(await db.db.select().from(schema.kcGroup)).toEqual([])
      expect((await call('DELETE', `/v1/orgs/${ORG}/groups/${dev.json().id}`, ADMIN, 'admin')).statusCode).toBe(404)
      const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.action, 'group.create'))
      expect(log.map((l) => l.target)).toEqual(['dev', 'dev/backend'])
    })

    it('forgets department memberships when a member is removed', async () => {
      await reset()
      const g = (await call('POST', `/v1/orgs/${ORG}/groups`, ADMIN, 'admin', { name: 'ops' })).json()
      await call('PUT', `/v1/orgs/${ORG}/groups/${g.id}/members/${MEMBER}`, ADMIN, 'admin')
      await call('DELETE', `/v1/orgs/${ORG}/members/${MEMBER}`, ADMIN, 'admin')
      expect(await db.db.select().from(schema.kcGroupMember)).toEqual([])
    })
  })

  describe('presence', () => {
    /** A connected device that records what the server sends it. */
    const connect = (userId: string) => {
      const sent: string[] = []
      const socket = { readyState: 1, OPEN: 1, send: (m: string) => sent.push(m) }
      const remove = app.hub.add(userId, { socket: socket as never, deviceId: null })
      return { sent, remove }
    }
    const put = (uid: string, role: string, body: object) => call('PUT', `/v1/orgs/${ORG}/presence`, uid, role, body)
    const present = async (uid: string, role: string) => (await call('GET', `/v1/orgs/${ORG}/presence`, uid, role)).json() as { userId: string; state: string; projectId: string | null }[]

    it('shows who reports and is connected, and tells the others', async () => {
      await reset()
      const ada = connect(ADMIN)
      const max = connect(MEMBER)
      expect((await put(ADMIN, 'admin', { state: 'tracking', projectId: id(77) })).statusCode).toBe(204)
      expect(await present(MEMBER, 'member')).toMatchObject([{ userId: ADMIN, state: 'tracking', projectId: id(77) }])
      // Everyone in the organization hears about it, with the organization named.
      expect(max.sent).toContain(JSON.stringify({ type: 'presence', orgId: ORG }))
      // The same report again changes nothing, so nobody is bothered.
      const heard = max.sent.length
      await put(ADMIN, 'admin', { state: 'tracking', projectId: id(77) })
      expect(max.sent.length).toBe(heard)
      await put(ADMIN, 'admin', { state: 'away' })
      expect((await present(OWNER, 'owner'))[0]).toMatchObject({ state: 'away', projectId: null })
      expect(max.sent.length).toBe(heard + 1)
      ada.remove()
      max.remove()
    })

    it('forgets people whose last device is gone, and those who say they are offline', async () => {
      await reset()
      const owner = connect(OWNER)
      const admin = connect(ADMIN)
      await put(OWNER, 'owner', { state: 'online' })
      await put(ADMIN, 'admin', { state: 'online' })
      expect((await present(OWNER, 'owner')).map((p) => p.userId).sort()).toEqual([OWNER, ADMIN].sort())
      admin.remove() // no device left: not present, whatever they reported
      expect((await present(OWNER, 'owner')).map((p) => p.userId)).toEqual([OWNER])
      await put(OWNER, 'owner', { state: 'offline' })
      expect(await present(OWNER, 'owner')).toEqual([])
      owner.remove()
    })

    it('is for members only, and an unknown state is refused', async () => {
      await reset()
      expect((await call('GET', `/v1/orgs/${ORG}/presence`, OTHER, null)).statusCode).toBe(403)
      expect((await put(OTHER, null as never, { state: 'online' })).statusCode).toBe(403)
      expect((await put(ADMIN, 'admin', { state: 'dancing' })).statusCode).toBe(400)
    })
  })

  it('shows the audit log to owners and admins only, newest first', async () => {
    await reset()
    await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'a@example.com' })
    await call('POST', `/v1/orgs/${ORG}/invitations`, OWNER, 'owner', { email: 'b@example.com' })
    const res = await call('GET', `/v1/orgs/${ORG}/audit?limit=1`, ADMIN, 'admin')
    expect(res.statusCode).toBe(200)
    expect(res.json()).toMatchObject([{ action: 'member.invite', target: 'b@example.com', actorId: OWNER }])
    const older = await call('GET', `/v1/orgs/${ORG}/audit?before=${res.json()[0].id}`, ADMIN, 'admin')
    expect(older.json()[0].target).toBe('a@example.com')
    expect((await call('GET', `/v1/orgs/${ORG}/audit`, MANAGER, 'manager')).statusCode).toBe(403)
  })

  it('deletes an organization only for owners', async () => {
    await reset()
    expect((await call('DELETE', `/v1/orgs/${ORG}`, ADMIN, 'admin')).statusCode).toBe(403)
    expect((await call('DELETE', `/v1/orgs/${ORG}`, OWNER, 'owner')).statusCode).toBe(204)
    expect(kc.calls).toEqual([`deleteOrganization ${ORG}`])
    expect(await db.db.select().from(schema.organization).where(eq(schema.organization.id, ORG))).toEqual([])
  })
})
