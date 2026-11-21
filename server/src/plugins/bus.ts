// The bus between API replicas (docs/PLAN.md, scaling). Everything on it is a HINT that makes a replica look sooner
// than its timers would: "somebody pushed" (so connected devices pull) and "this cache entry is stale". The messages
// carry opaque ids only, never names or content, and receivers never trust them for authorization: the worst a forged
// message can do is a cache miss or an extra pull.
//
// Two implementations: in-process (one replica, tests) and PostgreSQL LISTEN/NOTIFY on one dedicated connection per
// replica (not one of the pools). Notifications are sent after the commit, fire-and-forget: a failed or slow notify
// never fails a request. When the listening connection drops and comes back, messages may have been missed, so the
// replica resynchronizes (tells its devices to pull, empties the caches).

import { randomUUID } from 'node:crypto'
import type { FastifyBaseLogger, FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import postgres from 'postgres'
import { dropOrganizationMemberships, dropMembership, dropRoles, clearAuthCaches } from '../modules/orgs/roles.ts'
import { clearAccountCaches, dropAccount } from './identity.ts'
import { type Invalidation, MAX_INV_USERS, onAnnounce } from './invalidation.ts'

export const BUS_CHANNEL = 'zeima_bus_v1'
export const BUS_VERSION = 1
/** NOTIFY carries at most 8000 bytes; stay well below. */
export const MAX_PAYLOAD_BYTES = 7_000
/** Most user ids in one `changes` message (150 * 38 bytes is about 5.7 KB). */
export const MAX_USERS_PER_MESSAGE = 150

/** `ping` is the bus checking its own listening connection (see `PgBus`); it has no other effect. */
export type BusMessage = { k: 'changes'; users: string[]; dev: string | null } | Invalidation | { k: 'ping' }

export interface Bus {
  readonly kind: 'memory' | 'pg'
  /** Random per process: a replica ignores its own messages. */
  readonly src: string
  /** Fire-and-forget; never throws. */
  publish(message: BusMessage): void
  /** Called with every valid message from another replica. Returns the function that removes the handler. */
  subscribe(handler: (message: BusMessage) => void): () => void
  /** Called when the connection was lost and came back: messages may have been missed. */
  onResync(handler: () => void): () => void
  status(): 'ok' | 'error'
  start(): Promise<void>
  close(): Promise<void>
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
const isId = (v: unknown): v is string => typeof v === 'string' && UUID.test(v)

export function encode(src: string, message: BusMessage): string {
  return JSON.stringify({ v: BUS_VERSION, src, ...message })
}

/** Parses and checks a payload; null for anything malformed, of another version, or of an unknown kind. */
export function decode(raw: string): { src: string; message: BusMessage } | null {
  if (raw.length > 8_000) return null
  let m: Record<string, unknown>
  try {
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return null
    m = parsed as Record<string, unknown>
  } catch {
    return null
  }
  if (m.v !== BUS_VERSION || typeof m.src !== 'string' || m.src.length < 1 || m.src.length > 64) return null
  const src = m.src
  if (m.k === 'changes') {
    if (!Array.isArray(m.users) || m.users.length === 0 || m.users.length > MAX_USERS_PER_MESSAGE || !m.users.every(isId)) return null
    if (m.dev !== null && m.dev !== undefined && !isId(m.dev)) return null
    return { src, message: { k: 'changes', users: m.users as string[], dev: (m.dev as string | null | undefined) ?? null } }
  }
  if (m.k === 'inv') {
    if (m.what === 'membership' && isId(m.user)) return { src, message: { k: 'inv', what: 'membership', user: m.user } }
    if (m.what === 'roles' && isId(m.org)) return { src, message: { k: 'inv', what: 'roles', org: m.org } }
    if (m.what === 'orgMemberships' && isId(m.org)) {
      const message: Invalidation = { k: 'inv', what: 'orgMemberships', org: m.org }
      if (m.users !== undefined) {
        if (!Array.isArray(m.users) || m.users.length > MAX_INV_USERS || !m.users.every(isId)) return null
        message.users = m.users as string[]
      }
      if (m.all !== undefined) {
        if (m.all !== true) return null
        message.all = true
      }
      return { src, message }
    }
    return null
  }
  if (m.k === 'ping') return { src, message: { k: 'ping' } }
  if (m.k === 'acct' && m.all === true) return { src, message: { k: 'acct', all: true } }
  if (m.k === 'acct' && isId(m.user)) return { src, message: { k: 'acct', user: m.user } }
  return null
}

/** Publishes "these users have changes", split so no message is too big. */
export function publishChanges(bus: Pick<Bus, 'publish'>, users: Iterable<string>, dev: string | null) {
  const all = [...new Set(users)]
  for (let i = 0; i < all.length; i += MAX_USERS_PER_MESSAGE) bus.publish({ k: 'changes', users: all.slice(i, i + MAX_USERS_PER_MESSAGE), dev })
}

/** Warns at most once per interval, with how many were swallowed since (a slow database must not flood the log). */
export class CoalescedWarn {
  private last = -Infinity
  private swallowed = 0
  private readonly log: Pick<FastifyBaseLogger, 'warn'>
  private readonly intervalMs: number
  constructor(log: Pick<FastifyBaseLogger, 'warn'>, intervalMs = 5_000) {
    this.log = log
    this.intervalMs = intervalMs
  }
  warn(msg: string, extra: Record<string, unknown> = {}) {
    const now = Date.now()
    if (now - this.last < this.intervalMs) {
      this.swallowed++
      return
    }
    this.last = now
    this.log.warn({ ...extra, ...(this.swallowed > 0 && { dropped: this.swallowed }) }, msg)
    this.swallowed = 0
  }
}

abstract class BaseBus {
  readonly src = randomUUID()
  protected readonly handlers = new Set<(message: BusMessage) => void>()
  protected readonly resyncs = new Set<() => void>()
  subscribe(handler: (message: BusMessage) => void) {
    this.handlers.add(handler)
    return () => void this.handlers.delete(handler)
  }
  onResync(handler: () => void) {
    this.resyncs.add(handler)
    return () => void this.resyncs.delete(handler)
  }
}

/** One replica: there is nobody to tell. */
export class MemoryBus extends BaseBus implements Bus {
  readonly kind = 'memory' as const
  publish() {}
  status() {
    return 'ok' as const
  }
  async start() {}
  async close() {}
}

export interface PgBusOptions {
  /** Direct (not pooled by a transaction-mode proxy) connection string. */
  url: string
  /** Sends one NOTIFY. Default: a dedicated single connection of the bus (never the application's pools). */
  notify?: (channel: string, payload: string) => Promise<void>
  log: Pick<FastifyBaseLogger, 'warn' | 'info' | 'debug'> & Partial<Pick<FastifyBaseLogger, 'error'>>
  /** Longest wait for the first LISTEN at start; it carries on in the background after that. */
  startTimeoutMs?: number
  /** A notification not done after this long is reported (it keeps counting as waiting until it settles). */
  notifyTimeoutMs?: number
  maxPendingNotifies?: number
  /** Milliseconds between pings of the bus to itself; 0 turns them off. Default 30 s. */
  pingIntervalMs?: number
  /** Longest wait for a ping to come back before the listening connection is replaced. Default 5 s. */
  pingTimeoutMs?: number
  /**
   * Client-side limit of one NOTIFY on the notifying connection: after it the statement is cancelled and the connection
   * replaced (no startup parameter and no SET, which a pooler may reject or not keep). Default 10 s.
   */
  notifyStatementTimeoutMs?: number
  /** Consecutive missed echoes after which the listener is no longer replaced every cycle. Default 3. */
  missesBeforeBackoff?: number
  /** First wait between replacements once backing off (doubles with each replacement actually made, not with skipped cycles). Default: the ping interval. */
  backoffBaseMs?: number
  /** Longest wait between replacements. Default 10 minutes. */
  backoffMaxMs?: number
}

export class PgBus extends BaseBus implements Bus {
  readonly kind = 'pg' as const
  private listener: postgres.Sql | null = null
  private notifier: postgres.Sql | null = null
  private listening = false
  private pingMissed = false
  private notifyFailed = false
  private misses = 0
  private nextReplaceAt = 0
  private loud = false
  private pingSeq = 0
  private pingInFlight = false
  private replaced = 0
  private everListened = false
  private closed = false
  private pending = 0
  private generation = 0
  private retry: ReturnType<typeof setTimeout> | null = null
  private retryDone: (() => void) | null = null
  private backoffStep = 0
  private pingTimer: ReturnType<typeof setInterval> | null = null
  private pongTimer: ReturnType<typeof setTimeout> | null = null
  private unlisten: (() => Promise<void>) | null = null
  private readonly warnDrop: CoalescedWarn
  private readonly o: PgBusOptions

  constructor(options: PgBusOptions) {
    super()
    this.o = options
    this.warnDrop = new CoalescedWarn(options.log)
  }

  status() {
    return this.listening && !this.pingMissed && !this.notifyFailed ? ('ok' as const) : ('error' as const)
  }

  /** How many times the listening connection was replaced because its pings did not come back. */
  get listenerReplacements() {
    return this.replaced
  }

  /** Notifications handed to the database that have not finished yet (never more than the cap). */
  get pendingNotifies() {
    return this.pending
  }

  /** The application name of the listening connection, for finding it in `pg_stat_activity`. */
  get applicationName() {
    return `zeima-bus-${this.src}`
  }

  /** The application name of the connection that sends notifications. */
  get notifierApplicationName() {
    return `zeima-notify-${this.src}`
  }

  async start() {
    // One more dedicated connection for sending, so a slow database queues statements here and not in the owner pool.
    if (!this.o.notify) this.notifier = this.makeNotifier()
    const first = this.connect()
    await Promise.race([first, new Promise<void>((resolve) => setTimeout(resolve, this.o.startTimeoutMs ?? 5_000).unref())])
    const every = this.o.pingIntervalMs ?? 30_000
    if (every > 0 && !this.closed) {
      this.pingTimer = setInterval(() => this.ping(), every)
      this.pingTimer.unref()
    }
  }

  /** Only plain startup parameters (application_name): a pooler may reject others. Bounded connect, TCP keepalive and lifetime, so a dead socket is not kept for long. */
  private makeNotifier() {
    return postgres(this.o.url, {
      max: 1,
      onnotice: () => {},
      connect_timeout: 10,
      keep_alive: 10,
      idle_timeout: 300,
      max_lifetime: 1_800,
      connection: { application_name: this.notifierApplicationName },
    })
  }

  private replaceNotifier(stuck: postgres.Sql) {
    if (this.closed || this.notifier !== stuck) return
    this.notifier = this.makeNotifier()
    void stuck.end({ timeout: 1 }).catch(() => {})
  }

  /** NOTIFY on the dedicated connection with a client-side time limit; on a timeout the statement is cancelled and the connection replaced. */
  private notifyOnNotifier(channel: string, text: string): Promise<void> {
    const notifier = this.notifier!
    const query = notifier`select pg_notify(${channel}, ${text})`
    return new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        void query.cancel().catch(() => {}) // rejects when its own connection fails or times out
        this.replaceNotifier(notifier)
        reject(new Error('notify timed out'))
      }, this.o.notifyStatementTimeoutMs ?? 10_000)
      timer.unref()
      query.then(
        () => {
          clearTimeout(timer)
          resolve()
        },
        (err) => {
          clearTimeout(timer)
          reject(err)
        },
      )
    })
  }

  /** One dedicated connection: postgres.js opens it for `listen` and reconnects it by itself (backoff), calling `onlisten` each time it is listening again. */
  private connect(): Promise<void> {
    const generation = ++this.generation
    this.listener = postgres(this.o.url, { max: 1, onnotice: () => {}, connection: { application_name: this.applicationName }, connect_timeout: 10 })
    return this.attempt(0, generation, this.listener)
  }

  private async attempt(retries: number, generation: number, listener: postgres.Sql): Promise<void> {
    if (this.closed || generation !== this.generation) return
    try {
      const { unlisten } = await listener.listen(
        BUS_CHANNEL,
        (payload) => this.receive(payload),
        () => {
          if (generation === this.generation) this.onListening()
        },
      )
      if (generation === this.generation) this.unlisten = unlisten
    } catch (err) {
      if (generation !== this.generation) return
      this.listening = false
      const wait = Math.min(30_000, 500 * 2 ** retries) * (0.5 + Math.random() / 2)
      this.o.log.warn({ err, retryInMs: Math.round(wait) }, 'bus: could not listen, retrying')
      await new Promise<void>((resolve) => {
        this.retryDone = resolve
        this.retry = setTimeout(resolve, wait)
        this.retry.unref()
      })
      return this.attempt(retries + 1, generation, listener)
    }
  }

  /** Ends a waiting retry now so the attempt that awaits it returns (its generation is stale) instead of leaking. */
  private cancelRetry() {
    if (this.retry) clearTimeout(this.retry)
    this.retry = null
    const done = this.retryDone
    this.retryDone = null
    done?.()
  }

  private onListening() {
    const again = this.everListened
    this.everListened = true
    this.listening = true
    if (!again) return
    this.o.log.warn('bus: listening again after a lost connection, resynchronizing')
    for (const h of this.resyncs) {
      try {
        h()
      } catch (err) {
        this.o.log.warn({ err }, 'bus: resync handler failed')
      }
    }
  }

  /**
   * Sends a ping and expects to hear it back: a listening connection can be lost without the driver noticing. The echo
   * is only awaited once the notify has succeeded; a notify that fails or stalls says nothing about the listener (it
   * only turns the status to error, and a stuck notifier of our own is replaced).
   */
  private ping() {
    if (this.closed || this.pongTimer || this.pingInFlight) return
    const done = this.send({ k: 'ping' })
    if (!done) return // not sent (too many waiting): nothing to expect
    const seq = ++this.pingSeq
    this.pingInFlight = true
    const wait = this.o.pingTimeoutMs ?? 5_000
    const stuck = this.notifier
    const guard = setTimeout(() => {
      if (this.pingSeq !== seq || !this.pingInFlight) return
      this.pingInFlight = false
      this.notifyBroken(new Error('ping notify did not finish in time'))
      if (stuck) this.replaceNotifier(stuck)
    }, wait)
    guard.unref()
    done.then(
      () => {
        clearTimeout(guard)
        if (this.pingSeq !== seq || !this.pingInFlight || this.closed) return
        this.pingInFlight = false
        this.pongTimer = setTimeout(() => {
          this.pongTimer = null
          void this.pongMissed()
        }, wait)
        this.pongTimer.unref()
      },
      () => {
        clearTimeout(guard)
        if (this.pingSeq === seq) this.pingInFlight = false
      },
    )
  }

  private notifyBroken(err: unknown) {
    this.notifyFailed = true
    this.warnDrop.warn('bus: could not notify', { err })
  }

  private async pongMissed() {
    if (this.closed) return
    this.misses++
    const limit = this.o.missesBeforeBackoff ?? 3
    if (!this.pingMissed) this.o.log.warn('bus: own ping did not come back, replacing the listening connection')
    this.pingMissed = true
    if (this.misses > limit) {
      if (!this.loud) {
        this.loud = true
        ;(this.o.log.error ?? this.o.log.warn)(
          { misses: this.misses },
          'bus: the replicas cannot hear each other: our own pings keep not coming back although they are sent. BUS_DATABASE_URL must be a direct session connection to PostgreSQL, not a transaction-mode pooler (PgBouncer): LISTEN needs a session. Replacing the listening connection less and less often until this is fixed.',
        )
      }
      if (Date.now() < this.nextReplaceAt) return // backing off; the status stays error
    }
    if (this.misses >= limit) {
      const base = this.o.backoffBaseMs ?? (this.o.pingIntervalMs || 30_000)
      this.nextReplaceAt = Date.now() + Math.min(this.o.backoffMaxMs ?? 600_000, base * 2 ** this.backoffStep++)
    }
    this.listening = false
    this.replaced++
    this.cancelRetry()
    const old = this.listener
    this.unlisten = null
    // The new connection listens first; its `onlisten` runs the resync (everListened is set).
    const next = this.connect()
    await old?.end({ timeout: 1 }).catch(() => {})
    await next
  }

  private receive(payload: string) {
    const decoded = decode(payload)
    if (!decoded) {
      this.o.log.debug('bus: ignored a malformed message')
      return
    }
    if (decoded.message.k === 'ping') {
      // Only our own pings count, and they do nothing but prove the connection listens.
      if (decoded.src === this.src) {
        if (this.pongTimer) clearTimeout(this.pongTimer)
        this.pongTimer = null
        if (this.pingMissed) this.o.log.info('bus: ping came back, the listening connection works')
        this.pingMissed = false
        this.misses = 0
        this.nextReplaceAt = 0
        this.backoffStep = 0
        this.loud = false
      }
      return
    }
    if (decoded.src === this.src) return
    for (const h of this.handlers) {
      try {
        h(decoded.message)
      } catch (err) {
        this.o.log.warn({ err }, 'bus: handler failed')
      }
    }
  }

  publish(message: BusMessage) {
    this.send(message)?.catch(() => {}) // already reported by `send`
  }

  /** The notification as a promise (rejected when it failed or timed out), or null when it was not handed over. */
  private send(message: BusMessage): Promise<void> | null {
    if (this.closed) return null
    const payload = encode(this.src, message)
    if (Buffer.byteLength(payload) > MAX_PAYLOAD_BYTES) {
      this.warnDrop.warn('bus: message too big, not sent', { k: message.k })
      return null
    }
    if (this.pending >= (this.o.maxPendingNotifies ?? 200)) {
      this.warnDrop.warn('bus: too many notifications waiting, some dropped (the timers still bound staleness)')
      return null
    }
    // Counts until the database call itself settles, not until a timeout gives up waiting: the statement is still queued.
    this.pending++
    const timer = setTimeout(() => this.warnDrop.warn('bus: a notification is slow'), this.o.notifyTimeoutMs ?? 2_000)
    timer.unref()
    const notify = this.o.notify ?? ((channel: string, text: string) => this.notifyOnNotifier(channel, text))
    let done: Promise<void>
    try {
      done = notify(BUS_CHANNEL, payload)
    } catch (err) {
      done = Promise.reject(err)
    }
    const settled = done.then(
      () => {
        this.notifyFailed = false
      },
      (err: unknown) => {
        this.notifyBroken(err)
        throw err
      },
    )
    settled
      .catch(() => {})
      .finally(() => {
        clearTimeout(timer)
        this.pending--
      })
    return settled
  }

  async close() {
    if (this.closed) return
    this.closed = true
    this.listening = false
    this.generation++
    this.cancelRetry()
    if (this.pingTimer) clearInterval(this.pingTimer)
    if (this.pongTimer) clearTimeout(this.pongTimer)
    try {
      await this.unlisten?.()
    } catch {
      // the connection may be gone already
    }
    await this.listener?.end({ timeout: 1 }).catch(() => {})
    await this.notifier?.end({ timeout: 1 }).catch(() => {})
  }
}

declare module 'fastify' {
  interface FastifyInstance {
    bus: Bus
    /** Publishes to the other replicas; never throws and never waits. */
    publishBus: (message: BusMessage) => void
  }
}

export const busPlugin = fp(
  async (app: FastifyInstance, options: { bus: Bus; resyncSpreadMs?: number; resyncMinIntervalMs?: number }) => {
    const { bus } = options
    const spreadMs = options.resyncSpreadMs ?? 10_000
    const minIntervalMs = options.resyncMinIntervalMs ?? 30_000
    // After a resync the devices are told to pull over a window, not all at once (every replica coming back at the same time would stampede the database).
    const timers = new Set<ReturnType<typeof setTimeout>>()
    let lastResync = -Infinity
    let trailing: ReturnType<typeof setTimeout> | null = null
    const tellDevices = () => {
      lastResync = Date.now()
      if (spreadMs <= 0) return app.hub.notifyAll()
      const slots = new Map<number, string[]>()
      for (const user of app.hub.connectedUsers()) {
        const delay = Math.round((spreadMs * (0.5 + Math.random())) / 100) * 100
        slots.set(delay, [...(slots.get(delay) ?? []), user])
      }
      for (const [delay, users] of slots) {
        const t = setTimeout(() => {
          timers.delete(t)
          app.hub.notify(users)
        }, delay)
        t.unref()
        timers.add(t)
      }
    }
    app.decorate('bus', bus)
    app.decorate('publishBus', (message: BusMessage) => {
      try {
        bus.publish(message)
      } catch (err) {
        app.log.warn({ err }, 'bus: could not publish')
      }
    })

    const offs = [
      // What this process invalidated, the others invalidate too.
      onAnnounce((message) => app.publishBus(message)),
      // Receivers only act locally (never announce again) and only drop cache entries or tell devices to pull.
      bus.subscribe((message) => {
        switch (message.k) {
          case 'changes':
            app.hub.notify(message.users, message.dev)
            break
          case 'inv':
            if (message.what === 'membership') dropMembership(message.user)
            else if (message.what === 'roles') dropRoles(message.org)
            else dropOrganizationMemberships(message.org, message.users, message.all)
            break
          case 'acct':
            if ('all' in message) clearAccountCaches()
            else dropAccount(message.user)
            break
        }
      }),
      bus.onResync(() => {
        clearAuthCaches()
        clearAccountCaches()
        // At most one round per interval; a resync inside it is deferred to the end of it, not lost.
        const wait = lastResync + minIntervalMs - Date.now()
        if (wait <= 0) tellDevices()
        else if (!trailing) {
          trailing = setTimeout(() => {
            trailing = null
            tellDevices()
          }, wait)
          trailing.unref()
        }
        // TODO(M2): resynchronize presence here.
      }),
    ]
    await bus.start()
    app.addHook('onClose', async () => {
      for (const off of offs) off()
      for (const t of timers) clearTimeout(t)
      if (trailing) clearTimeout(trailing)
      await bus.close()
    })
  },
  { name: 'bus', dependencies: ['realtime'] },
)
