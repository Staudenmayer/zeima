// The bus between API replicas: two apps against one database, talking through PostgreSQL LISTEN/NOTIFY.

import { eq, sql } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { type Invalidation, onAnnounce } from '../src/plugins/invalidation.ts'
import { forgetMemberships, forgetOrganizationMemberships, forgetMembership, forgetRoles, permissionsOfRole } from '../src/modules/orgs/roles.ts'
import { type BusMessage, BUS_CHANNEL, decode, encode, MAX_PAYLOAD_BYTES, MAX_USERS_PER_MESSAGE, MemoryBus, PgBus, publishChanges } from '../src/plugins/bus.ts'
import { forgetAccount } from '../src/plugins/identity.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { replica, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const ADA = id(1), BOB = id(2), LEAVER = id(3), OWNER = id(4)
const WS = id(100), DEVICE = id(900), OTHER = id(901)
const ORG = id(800)
const hlc = (wall: number) => `${String(wall).padStart(15, '0')}-000000-a`
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const until = async (fn: () => boolean | Promise<boolean>, ms = 3_000) => {
  const end = Date.now() + ms
  while (Date.now() < end) {
    if (await fn()) return true
    await sleep(25)
  }
  return fn()
}
const READY = JSON.stringify({ type: 'ready' })
const CHANGES = JSON.stringify({ type: 'changes' })

describe('bus messages', () => {
  it('round-trips each kind and keeps ids only', () => {
    const msgs: BusMessage[] = [
      { k: 'changes', users: [ADA, BOB], dev: DEVICE },
      { k: 'changes', users: [ADA], dev: null },
      { k: 'inv', what: 'membership', user: ADA },
      { k: 'inv', what: 'roles', org: ORG },
      { k: 'inv', what: 'orgMemberships', org: ORG },
      { k: 'acct', user: ADA },
    ]
    for (const m of msgs) expect(decode(encode('src-1', m))).toEqual({ src: 'src-1', message: m })
  })

  it('drops malformed, unknown and oversized messages', () => {
    const bad = [
      'nope',
      '[]',
      '{}',
      JSON.stringify({ v: 2, src: 'x', k: 'acct', user: ADA }),
      JSON.stringify({ v: 1, k: 'acct', user: ADA }),
      JSON.stringify({ v: 1, src: 'x', k: 'unknown' }),
      JSON.stringify({ v: 1, src: 'x', k: 'acct', user: 'not-a-uuid' }),
      JSON.stringify({ v: 1, src: 'x', k: 'inv', what: 'other', user: ADA }),
      JSON.stringify({ v: 1, src: 'x', k: 'changes', users: [], dev: null }),
      JSON.stringify({ v: 1, src: 'x', k: 'changes', users: ['a'], dev: null }),
      JSON.stringify({ v: 1, src: 'x', k: 'changes', users: [ADA], dev: 5 }),
      JSON.stringify({ v: 1, src: 'x', k: 'changes', users: Array.from({ length: MAX_USERS_PER_MESSAGE + 1 }, (_, i) => id(i)), dev: null }),
    ]
    for (const raw of bad) expect(decode(raw), raw).toBeNull()
  })

  it('splits a change for many users into messages that fit a notification', () => {
    const sent: BusMessage[] = []
    publishChanges({ publish: (m) => void sent.push(m) }, Array.from({ length: 400 }, (_, i) => id(10_000 + i)), DEVICE)
    expect(sent.map((m) => (m.k === 'changes' ? m.users.length : 0))).toEqual([150, 150, 100])
    for (const m of sent) expect(Buffer.byteLength(encode('x'.repeat(36), m))).toBeLessThan(MAX_PAYLOAD_BYTES)
  })

  it('defaults to the in-process bus in tests and PostgreSQL otherwise', () => {
    expect(loadConfig({ NODE_ENV: 'test' }).bus).toBe('memory')
    expect(loadConfig({ NODE_ENV: 'production' }).bus).toBe('pg')
    expect(loadConfig({ NODE_ENV: 'test', BUS: 'pg' }).bus).toBe('pg')
    expect(new MemoryBus().status()).toBe('ok')
  })
})

describe.skipIf(!(await reachable()))('two replicas on one database', () => {
  let database: Awaited<ReturnType<typeof createTestDatabase>>
  let a: Awaited<ReturnType<typeof replica>>
  let b: Awaited<ReturnType<typeof replica>>
  const extra: { close: () => Promise<unknown> }[] = []
  const seen: BusMessage[] = []

  const as = async (sub: string, extraHeaders: Record<string, string> = {}, groups: string[] = []) => ({ authorization: `Bearer ${await token({ sub, groups })}`, ...extraHeaders })
  const push = async (app: typeof a, sub: string, rowId: string, name: string, deviceId = DEVICE, wall = Date.now() % 1e12) =>
    app.inject({
      method: 'POST',
      url: '/v1/sync/push',
      headers: await as(sub),
      payload: { workspaceId: WS, deviceId, changes: [{ table: 'project', rowId, op: 'upsert', hlc: hlc(wall), fields: { name } }] },
    })
  const socket = async (app: typeof a, sub: string, deviceId?: string) => {
    const messages: string[] = []
    const ws = await app.injectWS(`/v1/sync/events${deviceId ? `?deviceId=${deviceId}` : ''}`, { headers: await as(sub) }, { onInit: (s) => s.on('message', (m) => messages.push(m.toString())) })
    return { ws, messages }
  }
  /** A notification from "another replica" (or a forger): straight into the channel. */
  const notify = (payload: string) => database.db.execute(sql`select pg_notify(${BUS_CHANNEL}, ${payload})`)
  const raw = (message: Record<string, unknown>) => JSON.stringify({ v: 1, src: 'forger', ...message })

  beforeAll(async () => {
    database = await createTestDatabase()
    a = await replica(database, { keycloakAdmin: new FakeKeycloak() })
    b = await replica(database)
    b.bus.subscribe((m) => void seen.push(m))
    await database.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    for (const u of [ADA, BOB, LEAVER, OWNER]) await database.db.insert(schema.kcUser).values({ id: u, email: `${u.slice(-1)}@example.com` })
    await database.db.insert(schema.kcOrgMember).values([
      { orgId: ORG, userId: LEAVER, role: 'member' },
      { orgId: ORG, userId: OWNER, role: 'owner' },
    ])
    expect((await a.inject({ method: 'POST', url: '/v1/workspaces', headers: await as(ADA), payload: { id: WS, name: 'Ada' } })).statusCode).toBe(200)
  })
  afterAll(async () => {
    for (const e of extra) await e.close()
    await a?.close()
    await b?.close()
    await database?.drop()
  })

  it('both replicas listen, each on its own connection, and report it', async () => {
    expect(a.bus.kind).toBe('pg')
    expect(a.bus.src).not.toBe(b.bus.src)
    const rows = await database.db.execute(sql`select count(*)::int as n from pg_stat_activity where application_name like 'zeima-bus-%' and datname = current_database()`)
    expect(Number((rows as unknown as { n: number }[])[0]!.n)).toBe(2)
    const ready = await b.inject('/readyz')
    expect(ready.statusCode).toBe(200)
    expect(ready.json().info).toEqual({ bus: 'ok' })
  })

  it('a push on one replica tells a device connected to the other, not the device that pushed', async () => {
    const other = await socket(b, ADA, OTHER)
    const sameDevice = await socket(b, ADA, DEVICE)
    const stranger = await socket(b, BOB)
    await sleep(50)
    expect(other.messages).toEqual([READY])
    expect((await push(a, ADA, id(200), 'Alpha')).statusCode).toBe(200)
    expect(await until(() => other.messages.length === 2)).toBe(true)
    expect(other.messages).toEqual([READY, CHANGES])
    await sleep(100)
    expect(sameDevice.messages).toEqual([READY])
    expect(stranger.messages).toEqual([READY])
    for (const s of [other, sameDevice, stranger]) s.ws.terminate()
  })

  it('a change that applies nothing says nothing on the bus', async () => {
    await push(a, ADA, id(201), 'Beta')
    await sleep(200)
    const before = seen.length
    await push(a, ADA, id(201), 'Older', DEVICE, 1)
    await sleep(150)
    expect(seen.length).toBe(before)
  })

  describe('cache invalidation', () => {
    it('an account switched off on one replica is refused by the other within a second, not after the cache time', async () => {
      const me = async (app: typeof a) => (await app.inject({ url: '/v1/me', headers: await as(BOB) })).statusCode
      expect(await me(b)).toBe(200) // B remembers: active
      await database.db.update(schema.kcUser).set({ active: false }).where(eq(schema.kcUser.id, BOB))
      expect(await me(b)).toBe(200) // still remembered (15 s)
      a.publishBus({ k: 'acct', user: BOB })
      expect(await until(async () => (await me(b)) === 401, 1_000)).toBe(true)
      await database.db.update(schema.kcUser).set({ active: true }).where(eq(schema.kcUser.id, BOB))
      forgetAccount(BOB)
      expect(await until(async () => (await me(b)) === 200, 1_000)).toBe(true)
    })

    it('a person removed from an organization on one replica loses access on the other at once', async () => {
      const members = async (app: typeof a) => (await app.inject({ url: `/v1/me`, headers: await as(LEAVER, { 'x-org-id': ORG }, [`/orgs/${ORG}/roles/member`]) })).statusCode
      expect(await members(b)).toBe(200)
      await database.db.delete(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, LEAVER))
      expect(await members(b)).toBe(200) // remembered for a few seconds
      a.publishBus({ k: 'inv', what: 'membership', user: LEAVER })
      expect(await until(async () => (await members(b)) === 403, 1_000)).toBe(true)
      await database.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: LEAVER, role: 'member' })
      a.publishBus({ k: 'inv', what: 'membership', user: LEAVER })
      expect(await until(async () => (await members(b)) === 200, 1_000)).toBe(true)
      // A whole organization's entries at once (a department deleted, the organization removed).
      await database.db.delete(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, LEAVER))
      expect(await members(b)).toBe(200)
      a.publishBus({ k: 'inv', what: 'orgMemberships', org: ORG })
      expect(await until(async () => (await members(b)) === 403, 1_000)).toBe(true)
      await database.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: LEAVER, role: 'member' })
      forgetMembership(LEAVER)
    })

    it('a bulk change that adds members is seen by the other replica at once, not after the cache time', async () => {
      const NEWBIE = id(5)
      await database.db.insert(schema.kcUser).values({ id: NEWBIE, email: 'newbie@example.com' })
      const me = async (app: typeof a) => (await app.inject({ url: '/v1/me', headers: await as(NEWBIE, { 'x-org-id': ORG }, [`/orgs/${ORG}/roles/member`]) })).statusCode
      // What the replica that made the change announces for 21 people (NEWBIE among them).
      const sent: Invalidation[] = []
      const off = onAnnounce((m) => void sent.push(m))
      forgetMemberships(ORG, [NEWBIE, ...Array.from({ length: 20 }, (_, i) => id(6_000 + i))])
      off()
      const bulk = sent.find((m) => m.k === 'inv' && m.what === 'orgMemberships')!
      expect(bulk).toMatchObject({ users: expect.arrayContaining([NEWBIE]) })
      await sleep(300) // the announcements above have reached b
      expect(await me(b)).toBe(403) // not a member yet; remembered as such
      await database.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: NEWBIE, role: 'member' })
      expect(await me(b)).toBe(403) // remembered for a few seconds
      a.publishBus(bulk)
      expect(await until(async () => (await me(b)) === 200, 1_000)).toBe(true)
      await database.db.delete(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, NEWBIE))
      forgetMembership(NEWBIE)
    })

    it('a changed role is seen by the other replica at once', async () => {
      const key = 'member'
      const before = await permissionsOfRole(database.db, ORG, key)
      await database.db.insert(schema.orgRole).values({ id: id(700), orgId: ORG, key, name: 'Member', description: '', permissions: { 'time.read': 'none' } as never })
      expect(await permissionsOfRole(database.db, ORG, key)).toEqual(before) // cached
      a.publishBus({ k: 'inv', what: 'roles', org: ORG })
      expect(await until(async () => JSON.stringify(await permissionsOfRole(database.db, ORG, key)) !== JSON.stringify(before), 1_000)).toBe(true)
      await database.db.delete(schema.orgRole).where(eq(schema.orgRole.id, id(700)))
      forgetRoles(ORG)
    })

    it('the forget functions announce themselves, and the receiver does not announce back', async () => {
      seen.length = 0
      forgetMembership(LEAVER)
      forgetRoles(ORG)
      forgetOrganizationMemberships(ORG)
      forgetAccount(BOB)
      expect(await until(() => new Set(seen.map((m) => JSON.stringify(m))).size === 4)).toBe(true)
      await sleep(200)
      // Each process sends once per call; nothing bounces (a loop would grow this without end).
      const count = seen.length
      await sleep(300)
      expect(seen.length).toBe(count)
    })

    it('an administrator switching an account off announces it', async () => {
      seen.length = 0
      const res = await a.inject({ method: 'PATCH', url: `/v1/orgs/${ORG}/members/${LEAVER}`, headers: await as(OWNER, {}, [`/orgs/${ORG}/roles/owner`]), payload: { active: false } })
      expect(res.statusCode).toBe(200)
      expect(await until(() => seen.some((m) => m.k === 'acct' && 'user' in m && m.user === LEAVER))).toBe(true)
      await database.db.update(schema.kcUser).set({ active: true }).where(eq(schema.kcUser.id, LEAVER))
    })
  })

  describe('messages are hints', () => {
    it('ignores malformed, wrong-version and own messages', async () => {
      const client = await socket(b, ADA, OTHER)
      seen.length = 0
      const before = client.messages.length
      const bad = [
        'plain text',
        '{"v":1',
        JSON.stringify({ v: 2, src: 'forger', k: 'changes', users: [ADA], dev: null }),
        raw({ k: 'changes', users: ['not-a-uuid'], dev: null }),
        raw({ k: 'changes', users: Array.from({ length: 151 }, () => ADA), dev: null }),
        raw({ k: 'surprise' }),
        // B's own message, as if it had come back.
        JSON.stringify({ v: 1, src: b.bus.src, k: 'changes', users: [ADA], dev: null }),
        JSON.stringify({ v: 1, src: b.bus.src, k: 'acct', user: ADA }),
      ]
      for (const p of bad) await notify(p)
      await sleep(300)
      expect(seen).toEqual([])
      expect(client.messages.length).toBe(before)
      // A good one from "another replica" does get through.
      await notify(raw({ k: 'changes', users: [ADA], dev: null }))
      expect(await until(() => client.messages.length === before + 1)).toBe(true)
      client.ws.terminate()
    })

    it('a forged invalidation costs a cache miss, never access', async () => {
      const me = async () => (await b.inject({ url: '/v1/me', headers: await as(LEAVER, { 'x-org-id': ORG }, [`/orgs/${ORG}/roles/admin`]) })).statusCode
      // The token claims admin, the database says member (and the owner switched the account on again above).
      await database.db.update(schema.kcUser).set({ active: true }).where(eq(schema.kcUser.id, LEAVER))
      expect(await me()).toBe(200)
      await database.db.delete(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, LEAVER))
      for (let i = 0; i < 3; i++) {
        await notify(raw({ k: 'inv', what: 'membership', user: LEAVER }))
        await notify(raw({ k: 'inv', what: 'orgMemberships', org: ORG }))
        await notify(raw({ k: 'acct', user: LEAVER }))
        await notify(raw({ k: 'inv', what: 'roles', org: ORG }))
      }
      await sleep(300)
      // The database decides: the person is not a member, however many "they are" messages arrive.
      expect(await me()).toBe(403)
      await database.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: LEAVER, role: 'member' })
      forgetMembership(LEAVER)
    })

    it('a notification that fails does not fail the push', async () => {
      const failing = new PgBus({ url: database.url, notify: () => Promise.reject(new Error('notify refused')), log: { warn: () => {}, info: () => {}, debug: () => {} } })
      const c = await replica(database, { bus: failing })
      extra.push(c)
      const throwing = await replica(database, {
        bus: Object.assign(new MemoryBus(), {
          publish() {
            throw new Error('boom')
          },
        }),
      })
      extra.push(throwing)
      for (const app of [c, throwing]) {
        const res = await push(app, ADA, id(250 + extra.length), 'Fine')
        expect(res.statusCode).toBe(200)
        expect(res.json().results).toEqual([{ status: 'applied' }])
      }
      const slow = await replica(database, { bus: new PgBus({ url: database.url, notify: () => new Promise(() => {}), notifyTimeoutMs: 50, log: { warn: () => {}, info: () => {}, debug: () => {} } }) })
      extra.push(slow)
      const t = Date.now()
      expect((await push(slow, ADA, id(260), 'Quick')).statusCode).toBe(200)
      expect(Date.now() - t).toBeLessThan(1_000)
    })
  })

  it('a change for many users reaches all of them through several messages', async () => {
    const users = Array.from({ length: 320 }, (_, i) => id(20_000 + i))
    const sockets = await Promise.all(users.map((u) => socket(b, u)))
    await sleep(50)
    seen.length = 0
    publishChanges(a.bus, users, null)
    expect(await until(() => sockets.every((s) => s.messages.length === 2), 5_000)).toBe(true)
    expect(seen.map((m) => (m.k === 'changes' ? m.users.length : 0)).sort((x, y) => y - x)).toEqual([150, 150, 20])
    for (const s of sockets) {
      expect(s.messages).toEqual([READY, CHANGES])
      s.ws.terminate()
    }
  })

  it('when the listening connection is killed the replica reconnects, resynchronizes and goes on', async () => {
    const client = await socket(b, ADA, OTHER)
    const me = async () => (await b.inject({ url: '/v1/me', headers: await as(BOB) })).statusCode
    expect(await me()).toBe(200) // B remembers BOB as active
    await database.db.update(schema.kcUser).set({ active: false }).where(eq(schema.kcUser.id, BOB))
    expect(await me()).toBe(200)
    await sleep(50)
    const before = client.messages.length

    const name = (b.bus as PgBus).applicationName
    const killed = await database.db.execute(sql`select pg_terminate_backend(pid) from pg_stat_activity where application_name = ${name}`)
    expect(killed.length).toBe(1)

    // Resync: the sockets are told to pull, the caches are empty (BOB's new state is seen at once).
    expect(await until(() => client.messages.length > before, 5_000)).toBe(true)
    expect(client.messages.at(-1)).toBe(CHANGES)
    expect(await me()).toBe(401)
    expect(await until(() => b.bus.status() === 'ok', 5_000)).toBe(true)
    const rows = await database.db.execute(sql`select count(*)::int as n from pg_stat_activity where application_name = ${name}`)
    expect(Number((rows as unknown as { n: number }[])[0]!.n)).toBe(1)

    // And messages flow again.
    const count = client.messages.length
    expect((await push(a, ADA, id(270), 'After')).statusCode).toBe(200)
    expect(await until(() => client.messages.length > count, 3_000)).toBe(true)
    expect(client.messages.at(-1)).toBe(CHANGES)
    await database.db.update(schema.kcUser).set({ active: true }).where(eq(schema.kcUser.id, BOB))
    client.ws.terminate()
  })

  it('closes its listening connection with the app', async () => {
    const c = await replica(database)
    const name = (c.bus as PgBus).applicationName
    const count = async () => Number(((await database.db.execute(sql`select count(*)::int as n from pg_stat_activity where application_name = ${name}`)) as unknown as { n: number }[])[0]!.n)
    expect(await count()).toBe(1)
    await c.close()
    expect(await until(async () => (await count()) === 0)).toBe(true)
  })
})
