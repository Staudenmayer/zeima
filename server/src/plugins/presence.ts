// Who is online, tracking or away in an organization (SPEC 5.10). Presence is only true while a device is connected and
// reporting, so it is worth nothing after a crash: it lives in UNLOGGED tables shared by the API replicas (drizzle/0017):
//
//   presence(org, user, state, project, since, seen)  the last report of a person in an organization
//   ws_conn(replica, user, n)                          which replica holds how many of the person's sockets
//   replica(id, heartbeat_at)                          the replicas alive (each refreshes its row every few seconds)
//
// A person is listed when their report is fresh (`TTL_MS`) AND some replica with a fresh heartbeat holds a socket of
// theirs. So a report made on one replica for a socket on another counts, and a replica that crashes takes its people
// away from the list within the heartbeat limit. Devices report on every change and about once a minute.
//
// The database is written off the hot path: opening or closing a socket only marks the person "dirty"; one loop
// per replica writes the current socket counts, and an error is logged (coalesced), never raised into the socket.
// Everything the replicas say to each other about presence is a hint (`{k:'presence', org}`): receivers just tell
// their own connected members to look again.

import { sql } from 'drizzle-orm'
import type { FastifyBaseLogger, FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import type { Db } from '../db/index.ts'
import { schema } from '../db/index.ts'
import { CoalescedWarn } from './bus.ts'

export const PRESENCE_STATES = ['online', 'in_meeting', 'traveling', 'away'] as const
export type PresenceState = (typeof PRESENCE_STATES)[number]

/** A device reports at least every minute. */
export const TTL_MS = 150_000

export interface PresenceTiming {
  /** How often a replica refreshes its row. Default 15 s. */
  heartbeatMs: number
  /** A replica whose heartbeat is older than this does not count as alive for the list. Default 45 s. */
  aliveMs: number
  /** A replica whose heartbeat is older than this is dead: the sweep removes it and its rows. Default 2 minutes. */
  deadMs: number
  /** About how often a replica sweeps (jittered 0.75x to 1.25x). Default 60 s. */
  sweepMs: number
  /** After a lost bus connection the local members are told over a window of 0.5x to 1.5x this. Default 10 s. */
  resyncSpreadMs: number
  /** After a database error the socket writes are retried after this long (nothing restarts them sooner). Default 2 s. */
  retryMs: number
  /** After the last local socket of a person closes while another replica still holds one: look again after this long. Default 2 s. */
  recheckMs: number
}

export const DEFAULT_TIMING: PresenceTiming = { heartbeatMs: 15_000, aliveMs: 45_000, deadMs: 120_000, sweepMs: 60_000, resyncSpreadMs: 10_000, retryMs: 2_000, recheckMs: 2_000 }

export interface PresenceItem {
  userId: string
  state: PresenceState
  projectId: string | null
  since: number
}

/** The sockets of this replica, as far as the board needs to know. */
export interface LocalSockets {
  count(userId: string): number
  connectedUsers(): string[]
}

export interface BoardOptions {
  db: Db | null
  replicaId: string
  sockets: LocalSockets
  log: Pick<FastifyBaseLogger, 'warn'>
  timing?: Partial<PresenceTiming>
  /** Presence of these organizations changed: tell their members (here and on the other replicas). */
  changed: (orgIds: string[]) => void
}

const CHUNK = 500
/** A list waits this long at most for the socket changes seen so far to be written. */
const LIST_FLUSH_MS = 200
const ids = (list: readonly string[]) => sql.join(list.map((id) => sql`${id}::uuid`), sql`, `)
const rowsOf = <T>(result: unknown) => result as unknown as T[]

export class PresenceBoard {
  readonly timing: PresenceTiming
  private readonly o: BoardOptions
  private readonly warn: CoalescedWarn
  private readonly dirty = new Set<string>()
  private pump: Promise<void> | null = null
  /** Everything connected here must be written again, and rows of people no longer connected removed (see `reassert`). */
  private full = false
  /** After a database error: no restart of the loop until the retry timer fires. */
  private backoff = false
  private closed = false
  private readonly inflight = new Set<Promise<unknown>>()
  private readonly recheckTimers = new Set<ReturnType<typeof setTimeout>>()
  private heartbeatTimer: ReturnType<typeof setTimeout> | null = null
  private sweepTimer: ReturnType<typeof setTimeout> | null = null
  private retryTimer: ReturnType<typeof setTimeout> | null = null

  constructor(options: BoardOptions) {
    this.o = options
    this.timing = { ...DEFAULT_TIMING, ...options.timing }
    this.warn = new CoalescedWarn(options.log)
  }

  private get db() {
    return this.o.db
  }

  /** `now() - ms` on the database clock: every replica compares heartbeats against the same clock. */
  private ago(ms: number) {
    return sql`clock_timestamp() - ${ms}::double precision * interval '1 millisecond'`
  }

  /** Some replica that is alive (heartbeat younger than `withinMs`) holds a socket of the person, other than `except`. */
  private live(userCol: ReturnType<typeof sql>, withinMs: number, except: string | null = null) {
    return sql`exists (select 1 from ws_conn w join replica r on r.id = w.replica_id
      where w.user_id = ${userCol} and r.heartbeat_at > ${this.ago(withinMs)}${except === null ? sql`` : sql` and w.replica_id <> ${except}`})`
  }

  // --- reports ---------------------------------------------------------------------------------------------------

  /** Records a report; returns whether something visible changed (a new person, another state or project, or a report after a silence). */
  async report(orgId: string, userId: string, state: PresenceState, projectId: string | null, now: number): Promise<boolean> {
    if (!this.db) return false
    const rows = rowsOf<{ changed: boolean }>(
      await this.db.execute(sql`
        with old as (select state, project_id, seen from presence where org_id = ${orgId} and user_id = ${userId}),
        up as (
          insert into presence (org_id, user_id, state, project_id, since, seen)
          values (${orgId}, ${userId}, ${state}, ${projectId}, ${now}, ${now})
          on conflict (org_id, user_id) do update set
            state = excluded.state, project_id = excluded.project_id, seen = excluded.seen,
            since = case when presence.state = excluded.state then presence.since else excluded.since end
          returning 1)
        select (not exists (select 1 from old)
          or exists (select 1 from old where state <> ${state} or project_id is distinct from ${projectId}::uuid or ${now}::bigint - seen > ${TTL_MS})) as changed`),
    )
    return rows[0]?.changed === true
  }

  /** Removes the person's report in one organization. Returns whether there was one. */
  async clear(orgId: string, userId: string): Promise<boolean> {
    if (!this.db) return false
    await this.flush(LIST_FLUSH_MS)
    const rows = rowsOf<unknown>(await this.db.execute(sql`delete from presence where org_id = ${orgId} and user_id = ${userId} returning 1`))
    return rows.length > 0
  }

  /** Who is present in the organization: fresh reports of members with a socket on a replica that is alive. */
  async list(orgId: string, now: number): Promise<PresenceItem[]> {
    if (!this.db) return []
    await this.flush(LIST_FLUSH_MS)
    const rows = rowsOf<{ user_id: string; state: PresenceState; project_id: string | null; since: string | number }>(
      await this.db.execute(sql`
        select p.user_id, p.state, p.project_id, p.since from presence p
        join kc_org_member m on m.org_id = p.org_id and m.user_id = p.user_id
        where p.org_id = ${orgId} and p.seen > ${now - TTL_MS} and ${this.live(sql`p.user_id`, this.timing.aliveMs)}
        order by p.user_id`),
    )
    return rows.map((r) => ({ userId: r.user_id, state: r.state, projectId: r.project_id, since: Number(r.since) }))
  }

  // --- sockets ---------------------------------------------------------------------------------------------------

  /** A socket of the person opened or closed (or the local count changed). Never waits, never throws. */
  touch(userId: string) {
    if (this.closed || !this.db) return
    this.dirty.add(userId)
    this.kick()
  }

  /** Starts the loop unless it runs or the database is in its retry delay. */
  private kick() {
    if (!this.backoff && !this.closed) this.pump ??= this.run()
  }

  /** Resolves when the socket changes seen so far are in the database (waits at most `maxMs`: a list is not held up by a storm). */
  async flush(maxMs = LIST_FLUSH_MS) {
    const pump = this.pump
    if (!pump) return
    let timer: ReturnType<typeof setTimeout> | undefined
    await Promise.race([pump, new Promise<void>((resolve) => (timer = setTimeout(resolve, maxMs)))])
    clearTimeout(timer)
  }

  /** The single writer of ws_conn: everything that changes this replica's rows goes through here, one step at a time. */
  private async run() {
    await null // `pump` is assigned before the first write
    let batch: string[] = []
    try {
      while ((this.dirty.size > 0 || this.full) && !this.closed) {
        try {
          if (this.full) {
            this.full = false
            try {
              await this.expandFull()
            } catch (err) {
              this.full = true
              throw err
            }
          }
          batch = [...this.dirty].slice(0, CHUNK)
          for (const u of batch) this.dirty.delete(u)
          await this.apply(batch)
        } catch (err) {
          this.warn.warn('presence: could not record sockets, will retry', { err })
          for (const u of batch) this.dirty.add(u)
          this.retryLater()
          return
        }
      }
    } finally {
      this.pump = null
    }
  }

  /** Marks everyone connected here dirty, and everyone with a row of this replica but no socket (a ghost). */
  private async expandFull() {
    const { replicaId, sockets } = this.o
    const connected = new Set(sockets.connectedUsers())
    const rows = rowsOf<{ user_id: string }>(await this.db!.execute(sql`select user_id from ws_conn where replica_id = ${replicaId}`))
    for (const r of rows) if (!connected.has(r.user_id)) this.dirty.add(r.user_id)
    for (const u of connected) this.dirty.add(u)
  }

  private retryLater() {
    this.backoff = true
    if (this.retryTimer || this.closed) return
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null
      this.backoff = false
      if ((this.dirty.size > 0 || this.full) && !this.closed) this.kick()
    }, this.timing.retryMs)
    this.retryTimer.unref()
  }

  /** Writes the current socket count of each person; those with none lose the row, and their presence goes if no other replica holds a socket. */
  private async apply(users: string[]) {
    const db = this.db!
    const { replicaId, sockets } = this.o
    const open = users.map((u) => ({ u, n: sockets.count(u) }))
    const up = open.filter((x) => x.n > 0)
    if (up.length > 0) {
      await db
        .insert(schema.wsConn)
        .values(up.map((x) => ({ replicaId, userId: x.u, n: x.n })))
        .onConflictDoUpdate({ target: [schema.wsConn.replicaId, schema.wsConn.userId], set: { n: sql`excluded.n` } })
    }
    const down = open.filter((x) => x.n === 0).map((x) => x.u)
    if (down.length > 0) await this.leave(down)
  }

  /** The people have no socket left on this replica: drop the rows, then their presence unless another replica holds one. */
  private async leave(users: string[]) {
    const db = this.db!
    const { replicaId, sockets } = this.o
    await db.execute(sql`delete from ws_conn where replica_id = ${replicaId} and user_id in (${ids(users)})`)
    // A person who came back in the meantime is touched again and written by the next round.
    const gone = users.filter((u) => sockets.count(u) === 0)
    if (gone.length === 0) return
    const orgs = rowsOf<{ org_id: string }>(
      await db.execute(sql`delete from presence p where p.user_id in (${ids(gone)}) and not ${this.live(sql`p.user_id`, this.timing.aliveMs)} returning p.org_id`),
    )
    this.announce(orgs.map((r) => r.org_id))
    // Two replicas that lose the last sockets of a person at the same moment can each still see the other's row and
    // keep the presence. Rows of other replicas left: look once more shortly, so the view heals in seconds, not at the sweep.
    const contested = rowsOf<{ user_id: string }>(await db.execute(sql`select distinct user_id from ws_conn where user_id in (${ids(gone)})`))
    if (contested.length > 0) this.recheckLater(contested.map((r) => r.user_id))
  }

  private recheckLater(users: string[]) {
    if (this.closed) return
    const t = setTimeout(() => {
      this.recheckTimers.delete(t)
      const p = this.recheck(users).catch((err) => this.warn.warn('presence: recheck failed', { err }))
      this.track(p)
    }, this.timing.recheckMs)
    t.unref()
    this.recheckTimers.add(t)
  }

  private async recheck(users: string[]) {
    if (this.closed || !this.db) return
    const gone = users.filter((u) => this.o.sockets.count(u) === 0)
    for (let i = 0; i < gone.length; i += CHUNK) {
      const chunk = gone.slice(i, i + CHUNK)
      const orgs = rowsOf<{ org_id: string }>(
        await this.db.execute(sql`delete from presence p where p.user_id in (${ids(chunk)}) and not ${this.live(sql`p.user_id`, this.timing.aliveMs)} returning p.org_id`),
      )
      this.announce(orgs.map((r) => r.org_id))
    }
  }

  private track<T>(p: Promise<T>): Promise<T> {
    this.inflight.add(p)
    const done = () => void this.inflight.delete(p)
    p.then(done, done)
    return p
  }

  private announce(orgIds: string[]) {
    const unique = [...new Set(orgIds)]
    if (unique.length > 0) this.o.changed(unique)
  }

  // --- liveness --------------------------------------------------------------------------------------------------

  /** Refreshes this replica's row; true when the row was missing (a truncated table, a sweep that was too eager). */
  private async beat(): Promise<boolean> {
    if (this.closed) return false
    const rows = rowsOf<{ inserted: boolean }>(
      await this.db!.execute(sql`insert into replica (id, heartbeat_at) values (${this.o.replicaId}, clock_timestamp())
        on conflict (id) do update set heartbeat_at = excluded.heartbeat_at returning (xmax = 0) as inserted`),
    )
    return rows[0]?.inserted === true
  }

  /**
   * Writes the socket count of everyone connected here (after rows may have been lost) and removes this replica's rows
   * of people who are not. It goes through the write loop like every other change, so it cannot resurrect a row that a
   * closing socket is deleting. Resolves when the loop is done (during a database retry delay: at once, the timer does it).
   */
  reassert(): Promise<void> {
    return this.track(this.doReassert())
  }

  private async doReassert() {
    if (!this.db || this.closed) return
    await this.beat()
    if (this.closed) return
    this.full = true
    this.kick()
    await this.pump
  }

  async start() {
    if (!this.db) return
    try {
      await this.reassert()
    } catch (err) {
      this.warn.warn('presence: could not register this replica', { err })
    }
    this.scheduleHeartbeat()
    this.scheduleSweep()
  }

  private scheduleHeartbeat() {
    if (this.closed) return
    this.heartbeatTimer = setTimeout(() => {
      this.track(
        (async () => {
          try {
            if (await this.beat()) await this.reassert()
          } catch (err) {
            this.warn.warn('presence: heartbeat failed', { err })
          }
          this.scheduleHeartbeat()
        })(),
      )
    }, this.timing.heartbeatMs)
    this.heartbeatTimer.unref()
  }

  private scheduleSweep() {
    if (this.closed) return
    this.sweepTimer = setTimeout(() => {
      this.track(
        (async () => {
          try {
            await this.sweep()
          } catch (err) {
            this.warn.warn('presence: sweep failed', { err })
          }
          this.scheduleSweep()
        })(),
      )
    }, this.timing.sweepMs * (0.75 + Math.random() / 2))
    this.sweepTimer.unref()
  }

  /**
   * Removes what nobody will: expired reports, dead replicas with their sockets, and the reports of people without
   * a socket on a replica that is not dead. Safe on every replica at once: each statement only deletes (RETURNING
   * says what this one removed), takes no long lock, and does nothing the second time.
   */
  async sweep(now = Date.now()) {
    if (!this.db || this.closed) return
    const db = this.db
    const orgs: string[] = []
    const take = (result: unknown) => void orgs.push(...rowsOf<{ org_id: string }>(result).map((r) => r.org_id))
    take(await db.execute(sql`delete from presence where seen < ${now - TTL_MS} returning org_id`))
    await db.execute(sql`delete from ws_conn w where not exists (select 1 from replica r where r.id = w.replica_id and r.heartbeat_at > ${this.ago(this.timing.deadMs)})`)
    await db.execute(sql`delete from replica where heartbeat_at < ${this.ago(this.timing.deadMs)}`)
    take(await db.execute(sql`delete from presence p where not ${this.live(sql`p.user_id`, this.timing.deadMs)} returning p.org_id`))
    this.announce(orgs)
  }

  /** Graceful shutdown: this replica's sockets are gone, so are its rows, and the people with no socket elsewhere. */
  async stop() {
    this.closed = true
    for (const t of [this.heartbeatTimer, this.sweepTimer, this.retryTimer, ...this.recheckTimers]) if (t) clearTimeout(t)
    this.heartbeatTimer = this.sweepTimer = this.retryTimer = null
    this.recheckTimers.clear()
    if (!this.db) return
    // A heartbeat, sweep, resync or write that is under way must not commit after the deletes below.
    await this.pump?.catch(() => {})
    await Promise.allSettled([...this.inflight])
    const { replicaId } = this.o
    try {
      const orgs = rowsOf<{ org_id: string }>(
        await this.db.execute(sql`
          with gone as (delete from ws_conn where replica_id = ${replicaId} returning user_id)
          delete from presence p using (select distinct user_id from gone) g
          where p.user_id = g.user_id and not ${this.live(sql`p.user_id`, this.timing.aliveMs, replicaId)}
          returning p.org_id`),
      )
      await this.db.execute(sql`delete from replica where id = ${replicaId}`)
      this.announce(orgs.map((r) => r.org_id))
    } catch (err) {
      this.o.log.warn({ err }, 'presence: could not remove this replica on shutdown')
    }
  }
}

declare module 'fastify' {
  interface FastifyInstance {
    presence: PresenceBoard
  }
}

export interface PresencePluginOptions {
  timing?: Partial<PresenceTiming>
}

/** Presence on the shared database: needs the bus (for the replica id and the hints) and the hub (the sockets). */
export const presencePlugin = fp(
  async (app: FastifyInstance, options: PresencePluginOptions) => {
    const board = new PresenceBoard({
      db: app.db ?? null,
      replicaId: app.bus.src,
      sockets: { count: (u) => app.hub.count(u), connectedUsers: () => app.hub.connectedUsers() },
      log: app.log,
      ...(options.timing && { timing: options.timing }),
      changed: (orgIds) => {
        for (const orgId of orgIds) void app.notifyPresence(orgId).catch(() => {})
      },
    })
    app.decorate('presence', board)
    app.hub.onChange = (userId) => board.touch(userId)

    // After a lost bus connection: presence hints may have been missed, and the rows may have been lost with the tables.
    const timers = new Set<ReturnType<typeof setTimeout>>()
    const off = app.bus.onResync(() => {
      void (async () => {
        try {
          await board.reassert()
          const users = app.hub.connectedUsers()
          if (users.length === 0 || !app.db) return
          const byOrg = new Map<string, string[]>()
          for (let i = 0; i < users.length; i += CHUNK) {
            const rows = rowsOf<{ org_id: string; user_id: string }>(
              await app.db.execute(sql`select org_id, user_id from kc_org_member where user_id in (${ids(users.slice(i, i + CHUNK))})`),
            )
            for (const r of rows) byOrg.set(r.org_id, [...(byOrg.get(r.org_id) ?? []), r.user_id])
          }
          const spread = board.timing.resyncSpreadMs
          for (const [orgId, members] of byOrg) {
            const t = setTimeout(() => {
              timers.delete(t)
              app.hub.notifyPresence(members, orgId)
            }, spread <= 0 ? 0 : Math.round(spread * (0.5 + Math.random())))
            t.unref()
            timers.add(t)
          }
        } catch (err) {
          app.log.warn({ err }, 'presence: resynchronization failed')
        }
      })()
    })

    await board.start()
    app.addHook('onClose', async () => {
      off()
      for (const t of timers) clearTimeout(t)
      app.hub.onChange = null
      await board.stop()
    })
  },
  { name: 'presence', dependencies: ['realtime', 'bus'] },
)
