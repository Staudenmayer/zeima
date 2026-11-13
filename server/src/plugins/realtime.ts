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
import { SECURITY } from './auth.ts'

export const TOKEN_EXPIRED = 4401

interface Client {
  socket: WebSocket
  deviceId: string | null
}

/** Who is connected, by user. */
export class Hub {
  private readonly clients = new Map<string, Set<Client>>()

  add(userId: string, client: Client) {
    let set = this.clients.get(userId)
    if (!set) {
      set = new Set()
      this.clients.set(userId, set)
    }
    set.add(client)
    return () => {
      set.delete(client)
      if (set.size === 0 && this.clients.get(userId) === set) this.clients.delete(userId)
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

  get size() {
    let n = 0
    for (const set of this.clients.values()) n += set.size
    return n
  }
}

declare module 'fastify' {
  interface FastifyInstance {
    hub: Hub
  }
}

export const realtimePlugin = fp(
  async (app: FastifyInstance) => {
    await app.register(websocket, { options: { maxPayload: 1024 } })
    const hub = new Hub()
    app.decorate('hub', hub)

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
          remove()
        })
        socket.send(JSON.stringify({ type: 'ready' }))
      },
    )

    app.addHook('onClose', async () => {
      // Close sockets so shutdown does not wait for devices.
      for (const client of app.websocketServer.clients) client.close(1001, 'server shutting down')
    })
  },
  { name: 'realtime', dependencies: ['auth'] },
)
