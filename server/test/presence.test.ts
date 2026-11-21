// Presence on the shared database: two (or three) API replicas against one test database with the PostgreSQL bus.

import { sql } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { schema } from '../src/db/index.ts'
import { BUS_CHANNEL, decode, encode, PgBus } from '../src/plugins/bus.ts'
import type { Db } from '../src/db/index.ts'
import { PresenceBoard, TTL_MS } from '../src/plugins/presence.ts'
import { replica, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const ADA = id(1), BOB = id(2), CAROL = id(3), STRANGER = id(4), LEAVER = id(5)
const ORG = id(800), OTHER_ORG = id(801)
const PRESENCE = JSON.stringify({ type: 'presence', orgId: ORG })
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const until = async (fn: () => boolean | Promise<boolean>, ms = 4_000) => {
  const end = Date.now() + ms
  while (Date.now() < end) {
    if (await fn()) return true
    await sleep(25)
  }
  return fn()
}
const FAST = { heartbeatMs: 100, aliveMs: 1_500, deadMs: 2_500, sweepMs: 300, resyncSpreadMs: 0 }
const quiet = { warn() {}, info() {}, debug() {}, error() {} }

/** A bus whose "connection came back" can be triggered by the test. */
class TestBus extends PgBus {
  resync() {
    for (const h of this.resyncs) h()
  }
}

describe('bus message', () => {
  it('presence carries one organization id and nothing else', () => {
    expect(decode(encode('s', { k: 'presence', org: ORG }))).toEqual({ src: 's', message: { k: 'presence', org: ORG } })
    for (const bad of [{ k: 'presence' }, { k: 'presence', org: 5 }, { k: 'presence', org: 'x' }, { k: 'presence', org: [ORG] }]) {
      expect(decode(JSON.stringify({ v: 1, src: 's', ...bad })), JSON.stringify(bad)).toBeNull()
    }
  })
})

describe.skipIf(!(await reachable()))('presence across replicas', () => {
  let database: Awaited<ReturnType<typeof createTestDatabase>>
  let a: Awaited<ReturnType<typeof replica>>
  let b: Awaited<ReturnType<typeof replica>>
  const extra: { close: () => Promise<unknown> }[] = []
  const sockets: { terminate: () => void }[] = []

  const headers = async (sub: string) => ({ authorization: `Bearer ${await token({ sub, groups: [`/orgs/${sub === STRANGER ? OTHER_ORG : ORG}/roles/member`] })}` })
  const connect = async (app: typeof a, sub: string) => {
    const messages: string[] = []
    const ws = await app.injectWS('/v1/sync/events', { headers: await headers(sub) }, { onInit: (s) => s.on('message', (m) => messages.push(m.toString())) })
    sockets.push(ws)
    return { ws, messages }
  }
  const put = async (app: typeof a, sub: string, body: object, org = ORG) => app.inject({ method: 'PUT', url: `/v1/orgs/${org}/presence`, headers: await headers(sub), payload: body })
  const list = async (app: typeof a, sub: string, org = ORG) => {
    const res = await app.inject({ url: `/v1/orgs/${org}/presence`, headers: await headers(sub) })
    return res.statusCode === 200 ? (res.json() as { userId: string }[]).map((p) => p.userId) : []
  }
  const status = async (app: typeof a, sub: string, org = ORG) => (await app.inject({ url: `/v1/orgs/${org}/presence`, headers: await headers(sub) })).statusCode
  const count = async (table: string, where = '') => Number(((await database.db.execute(sql.raw(`select count(*)::int as n from ${table} ${where}`))) as unknown as { n: number }[])[0]!.n)
  const clean = async () => {
    for (const s of sockets.splice(0)) s.terminate()
    await sleep(100)
    await database.db.execute(sql`delete from presence`)
    await database.db.execute(sql`delete from ws_conn`)
    await b.presence.reassert()
    await a.presence.reassert()
  }

  beforeAll(async () => {
    database = await createTestDatabase()
    a = await replica(database, { presenceTiming: FAST })
    b = await replica(database, { presenceTiming: FAST })
    await a.ready()
    await b.ready()
    await database.db.insert(schema.organization).values([{ id: ORG, name: 'Acme', createdAt: 1 }, { id: OTHER_ORG, name: 'Other', createdAt: 1 }])
    for (const u of [ADA, BOB, CAROL, STRANGER, LEAVER]) await database.db.insert(schema.kcUser).values({ id: u, email: `${u.slice(-1)}@example.com` })
    await database.db.insert(schema.kcOrgMember).values([ADA, BOB, CAROL, LEAVER].map((userId) => ({ orgId: ORG, userId, role: 'member' })))
    await database.db.insert(schema.kcOrgMember).values({ orgId: OTHER_ORG, userId: STRANGER, role: 'member' })
  })
  afterAll(async () => {
    for (const s of sockets) s.terminate()
    for (const e of extra) await e.close()
    await a?.close()
    await b?.close()
    await database?.drop()
  })

  it('lists a report made on one replica for a person connected to the other, and tells the peers on both', async () => {
    const bob = await connect(b, BOB)
    const carol = await connect(a, CAROL)
    const ada = await connect(b, ADA)
    expect((await put(a, BOB, { state: 'online' })).statusCode).toBe(204)
    expect(await until(async () => (await list(a, CAROL)).join() === BOB)).toBe(true)
    expect(await list(b, ADA)).toEqual([BOB])
    expect(await until(() => carol.messages.includes(PRESENCE) && ada.messages.includes(PRESENCE) && bob.messages.includes(PRESENCE))).toBe(true)
    // The same report again changes nothing, so nobody is bothered.
    const heard = carol.messages.length
    await put(a, BOB, { state: 'online' })
    await sleep(150)
    expect(carol.messages.length).toBe(heard)
    await clean()
  })

  it('forgets a person when their last socket closes, and tells the peers on the other replica', async () => {
    const bob1 = await connect(b, BOB)
    const bob2 = await connect(b, BOB)
    const carol = await connect(a, CAROL)
    await put(b, BOB, { state: 'away' })
    await until(() => carol.messages.includes(PRESENCE))
    expect(await count('ws_conn', `where user_id = '${BOB}'`)).toBe(1)
    bob1.ws.terminate()
    await sleep(200)
    expect(await list(a, CAROL)).toEqual([BOB]) // one device left
    const heard = carol.messages.length
    bob2.ws.terminate()
    expect(await until(async () => (await list(a, CAROL)).length === 0)).toBe(true)
    expect(await until(() => carol.messages.length > heard)).toBe(true)
    expect(await count('presence')).toBe(0)
    expect(await count('ws_conn', `where user_id = '${BOB}'`)).toBe(0)
    await clean()
  })

  it('does not drop a person who still has a socket on another replica', async () => {
    const onA = await connect(a, BOB)
    const onB = await connect(b, BOB)
    await put(a, BOB, { state: 'online' })
    onB.ws.terminate()
    await sleep(300)
    expect(await list(a, CAROL)).toEqual([BOB])
    onA.ws.terminate()
    expect(await until(async () => (await list(a, CAROL)).length === 0)).toBe(true)
    await clean()
  })

  it('hides the people of a replica that crashed, and the sweep removes its rows', async () => {
    // Registers once, then never beats again, and never cleans up: a replica that died.
    const c = await replica(database, { presenceTiming: { ...FAST, heartbeatMs: 3_600_000, sweepMs: 3_600_000 } })
    extra.push(c)
    await c.ready()
    await connect(c, ADA)
    await put(c, ADA, { state: 'online' })
    const keeper = await connect(a, BOB)
    await put(a, BOB, { state: 'online' })
    expect((await list(a, CAROL)).sort()).toEqual([ADA, BOB].sort())
    // After the alive limit it is not listed any more, although its rows are still there.
    expect(await until(async () => (await list(a, CAROL)).join() === BOB, 5_000)).toBe(true)
    // After the dead limit a sweep (run by A and B on their own) takes the rows away.
    expect(await until(async () => (await count('presence', `where user_id = '${ADA}'`)) === 0 && (await count('replica', `where id = '${c.bus.src}'`)) === 0 && (await count('ws_conn', `where replica_id = '${c.bus.src}'`)) === 0, 8_000)).toBe(true)
    expect(await list(a, CAROL)).toEqual([BOB])
    keeper.ws.terminate()
    await clean()
  })

  it('shows presence to members of the organization only', async () => {
    await connect(a, BOB)
    const stranger = await connect(a, STRANGER)
    await put(a, BOB, { state: 'online' })
    await sleep(200)
    expect(await status(a, STRANGER)).toBe(403)
    expect((await put(a, STRANGER, { state: 'online' })).statusCode).toBe(403)
    expect(await count('presence', `where user_id = '${STRANGER}'`)).toBe(0)
    expect(stranger.messages.filter((m) => m.includes('presence'))).toEqual([])
    // A person who is no longer a member is not shown, whatever they reported.
    const leaver = await connect(b, LEAVER)
    await put(b, LEAVER, { state: 'online' })
    expect((await list(a, CAROL)).sort()).toEqual([BOB, LEAVER].sort())
    await database.db.delete(schema.kcOrgMember).where(sql`${schema.kcOrgMember.userId} = ${LEAVER}`)
    expect(await list(a, CAROL)).toEqual([BOB])
    await database.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: LEAVER, role: 'member' })
    leaver.ws.terminate()
    await clean()
  })

  it('removes a report when the state is offline, and hides one older than the time to live', async () => {
    await connect(b, BOB)
    await connect(a, CAROL)
    await put(a, BOB, { state: 'traveling', projectId: id(77) })
    // A report does not wait for the socket rows of the two people to be written: a list waits only briefly.
    expect(await until(async () => (await list(b, CAROL)).join() === BOB)).toBe(true)
    await put(b, BOB, { state: 'offline' })
    expect(await list(a, CAROL)).toEqual([])
    expect(await count('presence')).toBe(0)
    await put(a, BOB, { state: 'online' })
    await database.db.execute(sql`update presence set seen = ${Date.now() - TTL_MS - 1_000}`)
    expect(await list(a, CAROL)).toEqual([])
    expect(await count('presence')).toBe(1) // hidden, not yet swept
    await a.presence.sweep()
    expect(await count('presence')).toBe(0)
    await clean()
  })

  it('writes the sockets again when a replica hears the bus come back after its rows were lost', async () => {
    const bus = new TestBus({ url: database.url, log: quiet })
    const d = await replica(database, { bus, presenceTiming: { ...FAST, heartbeatMs: 3_600_000, sweepMs: 3_600_000 } })
    extra.push(d)
    await d.ready()
    const ada = await connect(d, ADA)
    await connect(b, CAROL)
    await put(d, ADA, { state: 'online' })
    expect(await list(b, CAROL)).toEqual([ADA])
    await database.db.execute(sql`truncate presence, ws_conn, replica`)
    expect(await list(b, CAROL)).toEqual([])
    const heard = ada.messages.length
    bus.resync()
    expect(await until(async () => (await count('ws_conn', `where replica_id = '${bus.src}'`)) === 1)).toBe(true)
    expect(await until(() => ada.messages.length > heard && ada.messages.slice(heard).includes(PRESENCE))).toBe(true)
    // The device reports again (the client does it every minute) and is back.
    await put(d, ADA, { state: 'online' })
    expect(await list(b, CAROL)).toEqual([ADA])
    await clean()
    await d.close()
    extra.splice(extra.indexOf(d), 1)
  })

  it('does not close a socket when the database cannot record it', async () => {
    await database.db.execute(sql`alter table ws_conn rename to ws_conn_away`)
    let bob: Awaited<ReturnType<typeof connect>>
    try {
      bob = await connect(a, BOB)
      await sleep(300)
      expect(bob.ws.readyState).toBe(bob.ws.OPEN)
      expect(bob.messages).toEqual([JSON.stringify({ type: 'ready' })])
      // Still served: pings and messages from the server.
      a.hub.notifyPresence([BOB], ORG)
      expect(await until(() => bob.messages.includes(PRESENCE))).toBe(true)
    } finally {
      await database.db.execute(sql`alter table ws_conn_away rename to ws_conn`)
    }
    // The write is retried, so the person is recorded once the database is back.
    expect(await until(async () => (await count('ws_conn', `where user_id = '${BOB}'`)) === 1, 6_000)).toBe(true)
    expect(bob.ws.readyState).toBe(bob.ws.OPEN)
    await clean()
  })

  it('ignores forged and malformed presence messages', async () => {
    const carol = await connect(a, CAROL)
    const stranger = await connect(a, STRANGER)
    await sleep(100)
    const notify = (payload: string) => database.db.execute(sql`select pg_notify(${BUS_CHANNEL}, ${payload})`)
    const raw = (m: Record<string, unknown>) => JSON.stringify({ v: 1, src: 'forger', ...m })
    for (const bad of [raw({ k: 'presence' }), raw({ k: 'presence', org: 'x' }), raw({ k: 'presence', org: { a: 1 } }), raw({ k: 'presence', org: ORG, extra: 'x'.repeat(100) }).slice(0, 60), 'presence', JSON.stringify({ v: 1, k: 'presence', org: ORG })]) {
      await notify(bad)
    }
    await notify(raw({ k: 'presence', org: id(999) })) // an organization nobody belongs to: nothing to tell
    await sleep(300)
    expect(carol.messages.filter((m) => m.includes('presence'))).toEqual([])
    expect(stranger.messages.filter((m) => m.includes('presence'))).toEqual([])
    // A well-formed forgery is only a hint: members look again, nothing changes, an outsider hears nothing.
    await notify(raw({ k: 'presence', org: ORG }))
    expect(await until(() => carol.messages.includes(PRESENCE))).toBe(true)
    expect(stranger.messages.filter((m) => m.includes('presence'))).toEqual([])
    expect(await count('presence')).toBe(0)
    expect((await a.inject('/readyz')).statusCode).toBe(200)
    await clean()
  })

  it('removes its rows on a graceful shutdown and tells the others', async () => {
    const e = await replica(database, { presenceTiming: FAST })
    await e.ready()
    const ada = await connect(e, ADA)
    const bobElsewhere = await connect(a, BOB)
    const carol = await connect(a, CAROL)
    await put(e, ADA, { state: 'online' })
    await put(a, BOB, { state: 'online' })
    expect((await list(a, CAROL)).sort()).toEqual([ADA, BOB].sort())
    expect(await count('replica', `where id = '${e.bus.src}'`)).toBe(1)
    const heard = carol.messages.length
    await e.close()
    expect(await count('replica', `where id = '${e.bus.src}'`)).toBe(0)
    expect(await count('ws_conn', `where replica_id = '${e.bus.src}'`)).toBe(0)
    expect(await count('presence', `where user_id = '${ADA}'`)).toBe(0)
    expect(await count('presence', `where user_id = '${BOB}'`)).toBe(1) // another replica's person stays
    expect(await until(() => carol.messages.length > heard)).toBe(true)
    expect(await list(a, CAROL)).toEqual([BOB])
    ada.ws.terminate()
    bobElsewhere.ws.terminate()
    await clean()
  })
})

// The board on its own (no HTTP, no sockets): the database is wrapped so a test decides when a write lands.
describe.skipIf(!(await reachable()))('presence board races', () => {
  let database: Awaited<ReturnType<typeof createTestDatabase>>
  const boards: PresenceBoard[] = []
  const hooks: { insert: () => Promise<unknown>; execute: () => Promise<unknown> } = { insert: async () => {}, execute: async () => {} }

  /** The query builder, whose execution (`then`) first waits for `hook`. */
  const gated = <T extends object>(q: T, hook: () => Promise<unknown>): T =>
    new Proxy(q, {
      get(t, p) {
        if (p === 'then') return (res: never, rej: never) => hook().then(() => (t as unknown as Promise<unknown>).then(res, rej), rej)
        const v = (t as Record<string | symbol, unknown>)[p]
        return typeof v === 'function' ? (...a: unknown[]) => gated((v as (...x: unknown[]) => object).apply(t, a), hook) : v
      },
    })
  const wrapped = (): Db =>
    new Proxy(database.db, {
      get(t, p) {
        const v = (t as unknown as Record<string | symbol, unknown>)[p]
        if (p === 'insert') return (...a: unknown[]) => gated((v as (...x: unknown[]) => object).apply(t, a), () => hooks.insert())
        if (p === 'execute')
          return async (...a: unknown[]) => {
            await hooks.execute()
            return (v as (...x: unknown[]) => unknown).apply(t, a)
          }
        return typeof v === 'function' ? (v as (...x: unknown[]) => unknown).bind(t) : v
      },
    })

  /** A person count the test controls; `onCount` runs inside the first read of one person's count. */
  const fakeSockets = () => {
    const n = new Map<string, number>()
    const s = {
      n,
      onCount: null as null | (() => void),
      count(u: string) {
        const v = n.get(u) ?? 0
        const hook = s.onCount
        s.onCount = null
        hook?.()
        return v
      },
      connectedUsers: () => [...n].filter(([, c]) => c > 0).map(([u]) => u),
    }
    return s
  }
  const make = (sockets: ReturnType<typeof fakeSockets>, timing: object = {}, changed: (o: string[]) => void = () => {}, replicaId = `r-${boards.length}-${Math.random()}`) => {
    const board = new PresenceBoard({
      db: wrapped(),
      replicaId,
      sockets,
      log: quiet,
      timing: { heartbeatMs: 3_600_000, sweepMs: 3_600_000, resyncSpreadMs: 0, ...timing },
      changed,
    })
    boards.push(board)
    return { board, replicaId }
  }
  const count = async (table: string, where = '') => Number(((await database.db.execute(sql.raw(`select count(*)::int as n from ${table} ${where}`))) as unknown as { n: number }[])[0]!.n)
  const seed = async () => {
    await database.db.execute(sql`delete from presence`)
    await database.db.execute(sql`delete from ws_conn`)
    await database.db.execute(sql`delete from replica`)
    hooks.insert = async () => {}
    hooks.execute = async () => {}
  }
  const addPresence = (user: string) => database.db.execute(sql`insert into presence (org_id, user_id, state, since, seen) values (${ORG}, ${user}, 'online', ${Date.now()}, ${Date.now()})`)

  beforeAll(async () => {
    database = await createTestDatabase()
    await database.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    for (const u of [ADA, BOB, CAROL]) await database.db.insert(schema.kcUser).values({ id: u, email: `${u.slice(-1)}@example.com` })
    await database.db.insert(schema.kcOrgMember).values([ADA, BOB, CAROL].map((userId) => ({ orgId: ORG, userId, role: 'member' })))
  })
  afterAll(async () => {
    for (const b of boards) await b.stop()
    await database?.drop()
  })

  it('a reassert racing with the last socket closing leaves no ghost row', async () => {
    await seed()
    hooks.insert = () => sleep(150) // the write lands late
    const sockets = fakeSockets()
    sockets.n.set(BOB, 1)
    const { board, replicaId } = make(sockets)
    // The socket closes right after its count was read for the write.
    sockets.onCount = () => {
      sockets.n.set(BOB, 0)
      board.touch(BOB)
    }
    await board.reassert()
    await board.flush(5_000)
    expect(await count('ws_conn', `where replica_id = '${replicaId}'`)).toBe(0)
  })

  it('a reassert removes the rows of people who are no longer connected', async () => {
    await seed()
    const sockets = fakeSockets()
    sockets.n.set(ADA, 1)
    const { board, replicaId } = make(sockets)
    await database.db.execute(sql`insert into ws_conn (replica_id, user_id, n) values (${replicaId}, ${BOB}, 2)`)
    await addPresence(BOB)
    await board.reassert()
    expect(await count('ws_conn', `where replica_id = '${replicaId}' and user_id = '${ADA}'`)).toBe(1)
    expect(await count('ws_conn', `where user_id = '${BOB}'`)).toBe(0)
    expect(await count('presence', `where user_id = '${BOB}'`)).toBe(0)
  })

  it('stop() waits for a heartbeat under way: no replica row and no socket rows come back after it', async () => {
    await seed()
    const sockets = fakeSockets()
    sockets.n.set(BOB, 1)
    const { board, replicaId } = make(sockets, { heartbeatMs: 40 })
    await board.start()
    expect(await count('ws_conn', `where replica_id = '${replicaId}'`)).toBe(1)
    await database.db.execute(sql`delete from replica`) // the next beat inserts again and so reasserts
    let started = false
    hooks.execute = async () => {
      if (started) return
      started = true
      await sleep(300)
    }
    expect(await until(() => started)).toBe(true) // the heartbeat is in flight, held up
    hooks.execute = async () => {}
    await board.stop()
    await sleep(500) // the held-up heartbeat would land by now
    expect(await count('replica', `where id = '${replicaId}'`)).toBe(0)
    expect(await count('ws_conn', `where replica_id = '${replicaId}'`)).toBe(0)
  })

  it('heals when two replicas lose the last sockets of a person at once: the presence goes within the recheck, not at the sweep', async () => {
    await seed()
    const sockets = fakeSockets()
    sockets.n.set(BOB, 1)
    const told: string[][] = []
    const { board, replicaId } = make(sockets, { heartbeatMs: 100, recheckMs: 150 }, (o) => told.push(o))
    await board.start()
    // Another live replica holds a socket of BOB too, and has not deleted its row yet.
    await database.db.execute(sql`insert into replica (id, heartbeat_at) values ('other', clock_timestamp() + interval '1 hour')`)
    await database.db.execute(sql`insert into ws_conn (replica_id, user_id, n) values ('other', ${BOB}, 1)`)
    await addPresence(BOB)
    sockets.n.set(BOB, 0)
    board.touch(BOB)
    expect(await until(async () => (await count('ws_conn', `where replica_id = '${replicaId}'`)) === 0)).toBe(true)
    await sleep(50)
    expect(await count('presence', `where user_id = '${BOB}'`)).toBe(1) // the other replica's row was still there
    await database.db.execute(sql`delete from ws_conn where replica_id = 'other'`) // it closed its socket and saw ours
    expect(await until(async () => (await count('presence', `where user_id = '${BOB}'`)) === 0, 2_000)).toBe(true)
    expect(told.flat()).toContain(ORG)
  })

  it('report() does not wait for pending socket writes, and list() waits only briefly', async () => {
    await seed()
    hooks.insert = () => sleep(1_500)
    const sockets = fakeSockets()
    sockets.n.set(BOB, 1)
    const { board } = make(sockets)
    board.touch(BOB)
    await sleep(30)
    const t0 = Date.now()
    await board.report(ORG, BOB, 'online', null, Date.now())
    expect(Date.now() - t0).toBeLessThan(300)
    const t1 = Date.now()
    await board.list(ORG, Date.now())
    expect(Date.now() - t1).toBeLessThan(700)
    await board.flush(5_000)
  })

  it('after a database error the retry delay holds: later socket changes do not restart the writes sooner', async () => {
    await seed()
    let attempts = 0
    let failing = true
    hooks.insert = async () => {
      attempts++
      if (failing) throw new Error('database down')
    }
    const sockets = fakeSockets()
    sockets.n.set(BOB, 1)
    const { board, replicaId } = make(sockets, { retryMs: 500 })
    board.touch(BOB)
    expect(await until(() => attempts === 1)).toBe(true)
    await sleep(50)
    for (let i = 0; i < 20; i++) board.touch(i % 2 ? BOB : CAROL)
    await sleep(150)
    expect(attempts).toBe(1)
    failing = false
    sockets.n.set(CAROL, 1)
    expect(await until(async () => (await count('ws_conn', `where replica_id = '${replicaId}'`)) === 2, 3_000)).toBe(true)
    expect(attempts).toBe(2)
  })
})
