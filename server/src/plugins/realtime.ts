// "Changes available" channel (SPEC §6.4): devices hold a WebSocket open and
// are told when someone pushed, so they pull at once instead of on the next
// poll. The message carries no data; the device pulls.
//
//   GET /v1/sync/events?deviceId=<uuid>   (WebSocket; same bearer token as the REST API)
//
// The token is checked when the socket opens and the socket closes when the
// token expires (code 4401); the device reconnects with a fresh one.

import websocket from '@fastify/websocket'
import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { Type } from 'typebox'
import type { WebSocket } from 'ws'
import { schema } from '../db/index.ts'
import { SECURITY } from './auth.ts'
import { eq } from 'drizzle-orm'

export const TOKEN_EXPIRED = 4401
/** Close code 1012: the server is restarting, reconnect. */
export const SERVICE_RESTART = 1012
/** Most devices one account keeps connected; the oldest gives way (a dead connection must not block a reconnect). */
export const MAX_SOCKETS_PER_USER = 10

interface Client {
  socket: WebSocket
  deviceId: string | null
}

/** Who is connected, by user. */
export class Hub {
  private readonly clients = new Map<string, Set<Client>>()
  /** Called after a person's set of sockets changed (opened, closed). Must not throw or wait: presence records it in the background. */
  onChange: ((userId: string) => void) | null = null

  private changed(userId: string) {
    try {
      this.onChange?.(userId)
    } catch {
      // a failing observer never touches the socket
    }
  }

  add(userId: string, client: Client) {
    let set = this.clients.get(userId)
    if (!set) {
      set = new Set()
      this.clients.set(userId, set)
    }
    while (set.size >= MAX_SOCKETS_PER_USER) {
      const oldest = set.values().next().value as Client
      set.delete(oldest)
      oldest.socket.close(4429, 'too many connections')
    }
    set.add(client)
    this.changed(userId)
    return () => {
      const had = set.delete(client)
      if (set.size === 0 && this.clients.get(userId) === set) this.clients.delete(userId)
      if (had) this.changed(userId)
    }
  }

  /** Tells the users' devices to pull; `exceptDevice` is the one that pushed. */
  notify(userIds: Iterable<string>, exceptDevice: string | null = null) {
    const message = JSON.stringify({ type: 'changes' })
    for (const userId of new Set(userIds)) {
      for (const client of this.clients.get(userId) ?? []) {
        if (exceptDevice !== null && client.deviceId === exceptDevice) continue
        if (client.socket.readyState === client.socket.OPEN) client.socket.send(message)
      }
    }
  }

  /** Tells every connected device to pull (after a lost bus connection, when messages may have been missed). */
  notifyAll() {
    this.notify(this.clients.keys())
  }

  /** The people with a connected device now. */
  connectedUsers(): string[] {
    return [...this.clients.keys()]
  }

  /** Whether one of the user's devices is connected. */
  has(userId: string) {
    return this.count(userId) > 0
  }

  /** How many devices of the user are connected here. */
  count(userId: string) {
    return this.clients.get(userId)?.size ?? 0
  }

  /** Tells the users' devices that presence in an organization changed. */
  notifyPresence(userIds: Iterable<string>, orgId: string) {
    const message = JSON.stringify({ type: 'presence', orgId })
    for (const userId of new Set(userIds)) {
      for (const client of this.clients.get(userId) ?? []) {
        if (client.socket.readyState === client.socket.OPEN) client.socket.send(message)
      }
    }
  }

  get size() {
    let n = 0
    for (const set of this.clients.values()) n += set.size
    return n
  }
}

declare module 'fastify' {
  interface FastifyInstance {
    hub: Hub
    /** Tells this replica's connected members of an organization that presence changed (what a bus hint from another replica does). */
    notifyPresenceLocal: (orgId: string) => Promise<void>
    /** Tells the members of an organization, on every replica, that presence changed. */
    notifyPresence: (orgId: string) => Promise<void>
    closeSocketsSpread: (windowMs: number) => Promise<void>
  }
}

export const realtimePlugin = fp(
  async (app: FastifyInstance) => {
    await app.register(websocket, { options: { maxPayload: 1024 } })
    const hub = new Hub()
    app.decorate('hub', hub)
    // Only organizations asked about at the same moment share one lookup; a flood of hints for one organization is one query at a time.
    const asking = new Map<string, boolean>()
    app.decorate('notifyPresenceLocal', async (orgId: string) => {
      if (!app.db || hub.size === 0) return
      if (asking.has(orgId)) {
        asking.set(orgId, true) // once more when the lookup in progress is done
        return
      }
      asking.set(orgId, false)
      try {
        do {
          asking.set(orgId, false)
          const members = await app.db.select({ id: schema.kcOrgMember.userId }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, orgId))
          hub.notifyPresence(members.map((m) => m.id), orgId)
        } while (asking.get(orgId))
      } finally {
        asking.delete(orgId)
      }
    })
    app.decorate('notifyPresence', async (orgId: string) => {
      app.publishBus?.({ k: 'presence', org: orgId }) // after the change is committed; the others only look again
      await app.notifyPresenceLocal(orgId)
    })

    app.get(
      '/v1/sync/events',
      {
        websocket: true,
        schema: {
          hide: true, // not an HTTP operation; described in docs/PLAN.md and here
          tags: ['sync'],
          summary: 'Changes-available channel (WebSocket)',
          security: SECURITY,
          querystring: Type.Object({ deviceId: Type.Optional(Type.String({ format: 'uuid' })) }),
        },
      },
      (socket, request) => {
        const user = request.user!
        const { deviceId } = request.query as { deviceId?: string }
        const remove = hub.add(user.id, { socket, deviceId: deviceId ?? null })
        const timer = setTimeout(
          () => socket.close(TOKEN_EXPIRED, 'token expired'),
          Math.max(0, user.expiresAt - Date.now()),
        )
        socket.on('close', () => {
          clearTimeout(timer)
          remove() // the hub tells presence, which forgets the person when no replica holds a device of theirs any more
        })
        socket.send(JSON.stringify({ type: 'ready' }))
      },
    )

    // Asks every device to reconnect (1012 "service restart"), each at a random moment within `windowMs`, so a
    // restart of one replica does not send all its devices back at the same instant. Resolves when all are closed.
    app.decorate('closeSocketsSpread', async (windowMs: number) => {
      const timers: Promise<void>[] = []
      for (const client of app.websocketServer.clients) {
        timers.push(
          new Promise<void>((resolve) => {
            setTimeout(() => {
              if (client.readyState === client.OPEN) client.close(SERVICE_RESTART, 'server restarting')
              resolve()
            }, Math.random() * Math.max(0, windowMs))
          }),
        )
      }
      await Promise.all(timers)
    })

    app.addHook('onClose', async () => {
      // Close what is left so shutdown does not wait for devices.
      for (const client of app.websocketServer.clients) client.close(SERVICE_RESTART, 'server restarting')
    })
  },
  { name: 'realtime', dependencies: ['auth'] },
)
