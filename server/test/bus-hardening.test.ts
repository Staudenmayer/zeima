// The bus under load and failure: reconcile does not flood it, a slow database cannot pile up notifications, a silently
import type postgres from 'postgres'
// lost listening connection is noticed, a resync does not stampede, and the lock timeout is put back as it was.

import { eq, sql } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { schema } from '../src/db/index.ts'
import { lockPushOrder } from '../src/db/locks.ts'
import { BULK_ANNOUNCE_USERS, forgetMemberships } from '../src/modules/orgs/roles.ts'
import { BULK_ACCOUNT_ANNOUNCE, forgetAccounts } from '../src/plugins/identity.ts'
import { reconcileOrganization } from '../src/modules/orgs/reconcile.ts'
import { type BusMessage, BUS_CHANNEL, CoalescedWarn, decode, encode, MemoryBus, PgBus } from '../src/plugins/bus.ts'
import { type Invalidation, onAnnounce } from '../src/plugins/invalidation.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { replica, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const ORG = id(800)
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const until = async (fn: () => boolean | Promise<boolean>, ms = 4_000) => {
  const end = Date.now() + ms
  while (Date.now() < end) {
    if (await fn()) return true
    await sleep(20)
  }
  return fn()
}
const silent = { warn: () => {}, info: () => {}, debug: () => {} }
const READY = JSON.stringify({ type: 'ready' })
const CHANGES = JSON.stringify({ type: 'changes' })

describe('coalesced warnings', () => {
  it('logs once per interval with the count of the swallowed ones', async () => {
    const lines: { obj: Record<string, unknown>; msg: string }[] = []
    const w = new CoalescedWarn({ warn: ((obj: Record<string, unknown>, msg: string) => void lines.push({ obj, msg })) as never }, 50)
    for (let i = 0; i < 10; i++) w.warn('dropped')
    expect(lines.length).toBe(1)
    await sleep(70)
    w.warn('dropped')
    expect(lines.length).toBe(2)
    expect(lines[1]!.obj.dropped).toBe(9)
  })
})

describe('announcing several people at once', () => {
  it('sends one message per person up to the limit, beyond it messages for the organization that name the people, and one "everybody" for a crowd', () => {
    const sent: Invalidation[] = []
    const off = onAnnounce((m) => void sent.push(m))
    const people = (n: number) => Array.from({ length: n }, (_, i) => id(i + 1))
    forgetMemberships(ORG, people(BULK_ANNOUNCE_USERS))
    expect(sent.length).toBe(BULK_ANNOUNCE_USERS)
    sent.length = 0
    forgetMemberships(ORG, people(BULK_ANNOUNCE_USERS + 1))
    expect(sent).toEqual([{ k: 'inv', what: 'orgMemberships', org: ORG, users: people(BULK_ANNOUNCE_USERS + 1) }])
    sent.length = 0
    forgetMemberships(ORG, people(250))
    expect(sent.length).toBe(3)
    expect(sent.flatMap((m) => ('users' in m && m.users) || [])).toEqual(people(250))
    for (const m of sent) expect(Buffer.byteLength(encode('x'.repeat(36), m))).toBeLessThan(7_000)
    sent.length = 0
    forgetMemberships(ORG, people(501))
    expect(sent).toEqual([{ k: 'inv', what: 'orgMemberships', org: ORG, all: true }])
    off()
  })

  it('a mass switch-off is one message, a small one is one per account', () => {
    const sent: Invalidation[] = []
    const off = onAnnounce((m) => void sent.push(m))
    forgetAccounts(Array.from({ length: BULK_ACCOUNT_ANNOUNCE }, (_, i) => id(i + 1)))
    expect(sent.length).toBe(BULK_ACCOUNT_ANNOUNCE)
    sent.length = 0
    forgetAccounts(Array.from({ length: BULK_ACCOUNT_ANNOUNCE + 1 }, (_, i) => id(i + 1)))
    expect(sent).toEqual([{ k: 'acct', all: true }])
    off()
  })

  it('validates the lists of a membership message on receipt', () => {
    const ok = (m: object) => decode(JSON.stringify({ v: 1, src: 's', k: 'inv', what: 'orgMemberships', org: ORG, ...m }))
    expect(ok({ users: [id(1)] })?.message).toEqual({ k: 'inv', what: 'orgMemberships', org: ORG, users: [id(1)] })
    expect(ok({ all: true })?.message).toEqual({ k: 'inv', what: 'orgMemberships', org: ORG, all: true })
    expect(ok({})?.message).toEqual({ k: 'inv', what: 'orgMemberships', org: ORG })
    expect(ok({ users: Array.from({ length: 101 }, (_, i) => id(i + 1)) })).toBeNull()
    expect(ok({ users: ['nope'] })).toBeNull()
    expect(ok({ users: 'x' })).toBeNull()
    expect(ok({ all: 'yes' })).toBeNull()
    expect(decode(JSON.stringify({ v: 1, src: 's', k: 'acct', all: true }))?.message).toEqual({ k: 'acct', all: true })
    expect(decode(JSON.stringify({ v: 1, src: 's', k: 'acct', all: 1 }))).toBeNull()
  })
})

describe.skipIf(!(await reachable()))('bus hardening', () => {
  let database: Awaited<ReturnType<typeof createTestDatabase>>
  const extra: { close: () => Promise<unknown> }[] = []
  const as = async (sub: string) => ({ authorization: `Bearer ${await token({ sub, groups: [] })}` })
  const socket = async (app: Awaited<ReturnType<typeof replica>>, sub: string) => {
    const messages: string[] = []
    await app.ready()
    const ws = await app.injectWS('/v1/sync/events', { headers: await as(sub) }, { onInit: (s) => s.on('message', (m) => messages.push(m.toString())) })
    return { ws, messages }
  }

  beforeAll(async () => {
    database = await createTestDatabase()
    await database.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
  })
  afterAll(async () => {
    for (const e of extra) await e.close()
    await database?.drop()
  })

  describe('reconcile', () => {
    it('announces nothing for an unchanged organization of 500 people, one message for a bulk change, one per person for a small one', { timeout: 30_000 }, async () => {
      const kc = new FakeKeycloak()
      kc.members = Array.from({ length: 500 }, (_, i) => ({ id: id(1_000 + i), email: `u${i}@acme.example`, name: `U${i}`, enabled: true }))
      kc.departments = [{ id: id(50), path: `/orgs/${ORG}/groups/dev`, memberIds: kc.members.slice(0, 100).map((m) => m.id) }]
      const sent: Invalidation[] = []
      const off = onAnnounce((m) => void sent.push(m))
      try {
        await reconcileOrganization(database.db, kc, ORG)
        // 500 people joined: a few messages for the organization that name them (not 500)
        expect(sent.length).toBe(5)
        expect(sent.every((m) => m.k === 'inv' && m.what === 'orgMemberships' && m.org === ORG)).toBe(true)
        sent.length = 0

        await reconcileOrganization(database.db, kc, ORG)
        expect(sent).toEqual([])

        // 30 people leave: one message, not 30.
        const leavers = kc.members.slice(0, 30).map((m) => m.id)
        kc.members = kc.members.slice(30)
        await reconcileOrganization(database.db, kc, ORG)
        expect(sent).toEqual([{ k: 'inv', what: 'orgMemberships', org: ORG, users: leavers }])
        sent.length = 0

        // One person changes role: one message for that person.
        kc.roles = { ...kc.roles, admin: [kc.members[0]!.id] }
        await reconcileOrganization(database.db, kc, ORG)
        expect(sent).toEqual([{ k: 'inv', what: 'membership', user: kc.members[0]!.id }])
        sent.length = 0

        // One person moves department.
        kc.departments = [{ id: id(50), path: `/orgs/${ORG}/groups/dev`, memberIds: kc.members.slice(0, 100).map((m) => m.id) }]
        await reconcileOrganization(database.db, kc, ORG)
        sent.length = 0
        kc.departments = [{ id: id(50), path: `/orgs/${ORG}/groups/dev`, memberIds: kc.members.slice(1, 100).map((m) => m.id) }]
        await reconcileOrganization(database.db, kc, ORG)
        expect(sent).toEqual([{ k: 'inv', what: 'membership', user: kc.members[0]!.id }])
        sent.length = 0

        // An account switched off is announced as an account, once; switched off still, never again.
        kc.members = kc.members.map((m, i) => (i === 5 ? { ...m, enabled: false } : m))
        await reconcileOrganization(database.db, kc, ORG)
        expect(sent).toEqual([{ k: 'acct', user: kc.members[5]!.id }])
        sent.length = 0
        await reconcileOrganization(database.db, kc, ORG)
        expect(sent).toEqual([])
      } finally {
        off()
      }
    })
  })

  describe('reconcile announcements, other cases', () => {
    const org = async (n: number) => {
      await database.db.insert(schema.organization).values({ id: id(n), name: `Org ${n}`, createdAt: 1 })
      return id(n)
    }
    const people = (from: number, n: number) => Array.from({ length: n }, (_, i) => ({ id: id(from + i), email: `p${from + i}@x.example`, name: `P${from + i}`, enabled: true }))
    const collect = () => {
      const sent: Invalidation[] = []
      return { sent, off: onAnnounce((m) => void sent.push(m)) }
    }
    const members = (sent: Invalidation[]) => sent.flatMap((m) => (m.k === 'inv' && m.what === 'membership' ? [m.user] : m.k === 'inv' && m.what === 'orgMemberships' ? (m.users ?? []) : [])).sort()

    it('a department renamed announces its members, once each', async () => {
      const orgId = await org(802)
      const kc = new FakeKeycloak()
      kc.members = people(2_000, 5)
      kc.departments = [{ id: id(52), path: `/orgs/${orgId}/groups/dev`, memberIds: kc.members.map((m) => m.id) }]
      await reconcileOrganization(database.db, kc, orgId)
      const { sent, off } = collect()
      try {
        await reconcileOrganization(database.db, kc, orgId)
        expect(sent).toEqual([])
        kc.departments = [{ id: id(52), path: `/orgs/${orgId}/groups/engineering`, memberIds: kc.members.map((m) => m.id) }]
        await reconcileOrganization(database.db, kc, orgId)
        expect(members(sent)).toEqual(kc.members.map((m) => m.id).sort())
        expect(sent.length).toBe(5)
      } finally {
        off()
      }
    })

    it('a mixed bulk change (joined, left, new role) names exactly the people it touched, newcomers included', async () => {
      const orgId = await org(803)
      const kc = new FakeKeycloak()
      kc.members = people(3_000, 30)
      await reconcileOrganization(database.db, kc, orgId)
      const { sent, off } = collect()
      try {
        const joined = people(3_100, 10)
        const left = kc.members.slice(0, 5).map((m) => m.id)
        const promoted = kc.members.slice(5, 13).map((m) => m.id)
        kc.members = [...kc.members.slice(5), ...joined]
        kc.roles = { ...kc.roles, admin: promoted }
        await reconcileOrganization(database.db, kc, orgId)
        expect(sent.length).toBe(1) // 23 people: one message for the organization
        expect(sent[0]).toMatchObject({ k: 'inv', what: 'orgMemberships', org: orgId })
        expect(members(sent)).toEqual([...left, ...promoted, ...joined.map((m) => m.id)].sort())
      } finally {
        off()
      }
    })

    it('a mass switch-off is announced as one account message', async () => {
      const orgId = await org(804)
      const kc = new FakeKeycloak()
      kc.members = people(4_000, 30)
      await reconcileOrganization(database.db, kc, orgId)
      const { sent, off } = collect()
      try {
        kc.members = kc.members.map((m) => ({ ...m, enabled: false }))
        await reconcileOrganization(database.db, kc, orgId)
        expect(sent).toEqual([{ k: 'acct', all: true }])
      } finally {
        off()
      }
    })

    it('a change an administrator makes after reconcile started reading is overwritten with an announcement', async () => {
      const orgId = await org(805)
      const kc = new FakeKeycloak()
      kc.members = people(5_000, 2)
      await reconcileOrganization(database.db, kc, orgId)
      const target = kc.members[0]!.id
      kc.members = [...kc.members, ...people(5_100, 1)] // a newcomer, so the seat check runs (after the first read, before the transaction)
      const { sent, off } = collect()
      try {
        await reconcileOrganization(database.db, kc, orgId, {
          seatLimit: async () => {
            await database.db.update(schema.kcOrgMember).set({ role: 'admin' }).where(eq(schema.kcOrgMember.userId, target))
            return null
          },
        })
        expect(members(sent)).toContain(target) // Keycloak's "member" replaced the "admin" just set
      } finally {
        off()
      }
    })
  })

  describe('notifications', () => {
    it('a stalled notifying connection never has more than the cap waiting, and requests are unaffected', async () => {
      const lines: string[] = []
      const bus = new PgBus({ url: database.url, log: { ...silent, warn: ((_o: unknown, m: string) => void lines.push(m)) as never }, pingIntervalMs: 0 })
      const app = await replica(database, { bus })
      extra.push(app)
      const notifier = (bus as unknown as { notifier: postgres.Sql }).notifier
      const stall = notifier`select pg_sleep(2)`.then(() => {}) // the bus's own connection is busy
      await sleep(100)
      for (let i = 0; i < 500; i++) bus.publish({ k: 'acct', user: id(i + 1) })
      expect(bus.pendingNotifies).toBe(200)
      const t = Date.now()
      await database.db.execute(sql`select 1`)
      expect((await app.inject({ url: '/readyz' })).statusCode).toBe(200)
      expect(Date.now() - t).toBeLessThan(1_000)
      expect(bus.pendingNotifies).toBe(200)
      // 300 were dropped, but the log has a line or two, not 300.
      expect(lines.filter((l) => /too many/.test(l)).length).toBeLessThanOrEqual(2)
      await stall
      expect(await until(() => bus.pendingNotifies === 0, 5_000)).toBe(true)
    })

    it('does not count a notification as done when only the timeout gave up on it', async () => {
      let release: () => void = () => {}
      const stuck = new Promise<void>((resolve) => (release = resolve))
      const bus = new PgBus({ url: database.url, notify: () => stuck, notifyTimeoutMs: 20, maxPendingNotifies: 3, log: silent, pingIntervalMs: 0 })
      for (let i = 0; i < 10; i++) bus.publish({ k: 'acct', user: id(i + 1) })
      await sleep(100) // past the timeout
      expect(bus.pendingNotifies).toBe(3)
      release()
      expect(await until(() => bus.pendingNotifies === 0)).toBe(true)
      await bus.close()
    })

    it('a failing cancel of a timed-out notification is no unhandled rejection; the notifier is still replaced', async () => {
      const unhandled: unknown[] = []
      const onUnhandled = (e: unknown) => void unhandled.push(e)
      process.on('unhandledRejection', onUnhandled)
      try {
        const bus = new PgBus({ url: database.url, notifyStatementTimeoutMs: 30, notifyTimeoutMs: 60, log: silent, pingIntervalMs: 0 })
        const internals = bus as unknown as { notifier: unknown }
        await bus.start()
        const query = Object.assign(new Promise(() => {}), { cancel: () => Promise.reject(new Error('cancel failed')) })
        const fake = Object.assign(() => query, { end: async () => {} })
        const real = internals.notifier as postgres.Sql
        internals.notifier = fake
        bus.publish({ k: 'acct', user: id(1) })
        expect(await until(() => internals.notifier !== fake)).toBe(true)
        expect(await until(() => bus.pendingNotifies === 0)).toBe(true)
        await sleep(100) // let a stray rejection surface
        expect(unhandled).toEqual([])
        await real.end({ timeout: 1 }).catch(() => {})
        await bus.close()
      } finally {
        process.off('unhandledRejection', onUnhandled)
      }
    })

    it('replacing the listener while it is retrying settles the old attempt', async () => {
      const bus = new PgBus({ url: 'postgres://u:p@127.0.0.1:1/db', log: silent, pingIntervalMs: 0 })
      const internals = bus as unknown as { connect: () => Promise<void>; cancelRetry: () => void; retryDone: unknown }
      const first = internals.connect()
      expect(await until(() => internals.retryDone !== null)).toBe(true)
      const second = internals.connect() // a new generation
      internals.cancelRetry()
      let settled = false
      void first.then(() => (settled = true))
      expect(await until(() => settled, 1_000)).toBe(true)
      await bus.close()
      await second
    })
  })

  describe('heartbeat', () => {
    it('pings change nothing: no cache is cleared, no device told, no subscriber called', async () => {
      const bus = new PgBus({ url: database.url, log: silent, pingIntervalMs: 40, pingTimeoutMs: 1_000 })
      const app = await replica(database, { bus })
      extra.push(app)
      const seen: BusMessage[] = []
      bus.subscribe((m) => void seen.push(m))
      const client = await socket(app, id(2))
      const BOB = id(2)
      const me = async () => (await app.inject({ url: '/v1/me', headers: await as(BOB) })).statusCode
      expect(await me()).toBe(200)
      await database.db.update(schema.kcUser).set({ active: false }).where(eq(schema.kcUser.id, BOB))
      await sleep(400) // many pings
      expect(await me()).toBe(200) // still remembered: nothing cleared the cache
      expect(client.messages).toEqual([READY])
      expect(seen).toEqual([])
      expect(bus.status()).toBe('ok')
      client.ws.terminate()
    })

    it('a listener that never hears the echo although the notify succeeds: status error, listener replaced, resync, recovery', { timeout: 20_000 }, async () => {
      let blackhole = false
      const db = database.db
      const bus = new PgBus({
        url: database.url,
        log: silent,
        pingIntervalMs: 80,
        pingTimeoutMs: 120,
        // A silently lost listener: what is sent never reaches it.
        notify: (channel, payload) => db.execute(sql`select pg_notify(${blackhole ? 'elsewhere' : channel}, ${payload})`).then(() => {}),
      })
      const app = await replica(database, { bus })
      extra.push(app)
      let resyncs = 0
      bus.onResync(() => void resyncs++)
      const client = await socket(app, id(3))
      const backend = async () => ((await database.db.execute(sql`select pid from pg_stat_activity where application_name = ${bus.applicationName}`)) as unknown as { pid: number }[]).map((r) => r.pid)
      expect(await until(async () => (await backend()).length === 1)).toBe(true)
      const [before] = await backend()
      expect(bus.status()).toBe('ok')

      blackhole = true
      expect(await until(() => bus.status() === 'error')).toBe(true)
      expect((await app.inject('/readyz')).json().info).toEqual({ bus: 'error' }) // reported, not gating
      expect((await app.inject('/readyz')).statusCode).toBe(200)
      expect(await until(() => resyncs >= 1)).toBe(true) // reconnected, resynchronized
      expect(await until(async () => (await backend()).length === 1 && (await backend())[0] !== before)).toBe(true)
      expect(client.messages.at(-1)).toBe(CHANGES)

      blackhole = false
      expect(await until(() => bus.status() === 'ok')).toBe(true)
      expect(await until(async () => (await app.inject('/readyz')).json().info.bus === 'ok', 8_000)).toBe(true) // /readyz answers from a 5 s cache
      client.ws.terminate()
    })

    const backendPids = async (bus: PgBus) =>
      ((await database.db.execute(sql`select pid from pg_stat_activity where application_name = ${bus.applicationName}`)) as unknown as { pid: number }[]).map((r) => r.pid)

    it('a notify that fails or stalls while the listener is healthy: status error, but no replacement and no resync', { timeout: 20_000 }, async () => {
      let mode: 'ok' | 'reject' | 'stall' = 'ok'
      const db = database.db
      const lines: string[] = []
      const bus = new PgBus({
        url: database.url,
        log: { ...silent, warn: ((_o: unknown, m: string) => void lines.push(m)) as never },
        pingIntervalMs: 100,
        pingTimeoutMs: 500,
        notify: (channel, payload) =>
          mode === 'reject' ? Promise.reject(new Error('database down')) : mode === 'stall' ? new Promise<void>(() => {}) : db.execute(sql`select pg_notify(${channel}, ${payload})`).then(() => {}),
      })
      let resyncs = 0
      bus.onResync(() => void resyncs++)
      await bus.start()
      expect(await until(() => bus.status() === 'ok')).toBe(true)
      const pids = await backendPids(bus)
      expect(pids.length).toBe(1)

      for (const broken of ['reject', 'stall'] as const) {
        mode = broken
        expect(await until(() => bus.status() === 'error')).toBe(true)
        await sleep(1_500) // several ping cycles
        expect(bus.status()).toBe('error')
        expect(bus.listenerReplacements).toBe(0)
        expect(resyncs).toBe(0)
        expect(await backendPids(bus)).toEqual(pids)
        mode = 'ok'
        expect(await until(() => bus.status() === 'ok')).toBe(true)
      }
      // Logged, but coalesced: a few lines for 20 failed pings, not one each.
      expect(lines.filter((l) => /could not notify/.test(l)).length).toBeLessThanOrEqual(8)
      await bus.close()
    })

    it('after repeated missed echoes it backs off, says once that BUS_DATABASE_URL must be a session connection, and recovers', { timeout: 20_000 }, async () => {
      let blackhole = true
      const db = database.db
      const errors: string[] = []
      const bus = new PgBus({
        url: database.url,
        log: { ...silent, error: ((_o: unknown, m: string) => void errors.push(m)) as never },
        pingIntervalMs: 40,
        pingTimeoutMs: 30,
        backoffBaseMs: 250,
        backoffMaxMs: 500,
        notify: (channel, payload) => db.execute(sql`select pg_notify(${blackhole ? 'elsewhere' : channel}, ${payload})`).then(() => {}),
      })
      await bus.start()
      await sleep(2_000) // 50 ping cycles; replacing every cycle would mean about 50 replacements
      expect(bus.status()).toBe('error')
      expect(bus.listenerReplacements).toBeGreaterThanOrEqual(2)
      expect(bus.listenerReplacements).toBeLessThanOrEqual(10)
      expect(errors.length).toBe(1)
      expect(errors[0]).toMatch(/BUS_DATABASE_URL/)
      expect(errors[0]).toMatch(/session/)
      blackhole = false
      expect(await until(() => bus.status() === 'ok', 10_000)).toBe(true) // the first echo resets everything
      await bus.close()
    })

    it('a ping from another replica is ignored', async () => {
      const bus = new PgBus({ url: database.url, log: silent, pingIntervalMs: 0 })
      await bus.start()
      const seen: BusMessage[] = []
      bus.subscribe((m) => void seen.push(m))
      await database.db.execute(sql`select pg_notify(${BUS_CHANNEL}, ${JSON.stringify({ v: 1, src: 'other', k: 'ping' })})`)
      await sleep(200)
      expect(seen).toEqual([])
      expect(bus.status()).toBe('ok')
      await bus.close()
    })
  })

  describe('resync', () => {
    class TestBus extends MemoryBus {
      fire() {
        for (const h of this.resyncs) h()
      }
    }

    it('spreads the pull messages over a window and runs at most one round per interval', async () => {
      const bus = new TestBus()
      const app = await replica(database, { bus }, { BUS: 'memory', BUS_RESYNC_SPREAD_MS: '400', BUS_RESYNC_MIN_INTERVAL_MS: '1500' })
      extra.push(app)
      const clients = await Promise.all(Array.from({ length: 40 }, (_, i) => socket(app, id(30_000 + i))))
      const arrivals: number[] = []
      for (const c of clients) c.ws.on('message', (m: Buffer) => m.toString() === CHANGES && arrivals.push(Date.now()))
      const t0 = Date.now()
      bus.fire()
      expect(arrivals.length).toBe(0) // not at once
      expect(await until(() => arrivals.length === 40, 1_500)).toBe(true)
      expect(Math.min(...arrivals) - t0).toBeGreaterThanOrEqual(150) // from 0.5x the window
      expect(Math.max(...arrivals) - Math.min(...arrivals)).toBeGreaterThan(100) // spread, not a burst
      expect(Math.max(...arrivals) - t0).toBeLessThan(900)

      // Three more resyncs inside the interval: one round, held back until the interval is over.
      arrivals.length = 0
      bus.fire()
      bus.fire()
      bus.fire()
      await sleep(900)
      expect(arrivals.length).toBe(0)
      expect(await until(() => arrivals.length === 40, 2_500)).toBe(true)
      await sleep(600)
      expect(arrivals.length).toBe(40) // exactly one round
      expect(Math.min(...arrivals) - t0).toBeGreaterThanOrEqual(1_500)
      for (const c of clients) c.ws.terminate()
    })
  })

  describe('lock timeout', () => {
    it('a timeout of 0 or none sets nothing (never "0ms"), a bad one is refused, and the pool is refused', async () => {
      await database.db.transaction(async (tx) => {
        await tx.execute(sql`set local lock_timeout = '7s'`)
        await lockPushOrder(tx, { timeoutMs: 0 })
        expect(await show(tx)).toBe('7s')
        await lockPushOrder(tx, { timeoutMs: 0.5 })
        expect(await show(tx)).toBe('7s')
      })
      await expect(database.db.transaction((tx) => lockPushOrder(tx, { timeoutMs: -1 }))).rejects.toThrow(RangeError)
      await expect(database.db.transaction((tx) => lockPushOrder(tx, { timeoutMs: Number.NaN }))).rejects.toThrow(RangeError)
      await expect(lockPushOrder(database.db)).rejects.toThrow(/transaction/)
    })

    const show = async (tx: { execute: typeof database.db.execute }) => ((await tx.execute(sql`show lock_timeout`)) as unknown as { lock_timeout: string }[])[0]!.lock_timeout

    it('is back at the previous value after the lock is taken, not reset to 0', async () => {
      await database.db.transaction(async (tx) => {
        await tx.execute(sql`set local lock_timeout = '7s'`)
        await lockPushOrder(tx, { timeoutMs: 1_000 })
        expect(await show(tx)).toBe('7s')
      })
      await database.db.transaction(async (tx) => {
        expect(await show(tx)).toBe('0')
        await lockPushOrder(tx, { timeoutMs: 1_000 })
        expect(await show(tx)).toBe('0')
      })
    })
  })
})
