// Workspaces and sync over HTTP against a real PostgreSQL (skipped when none
// is reachable; see db.test.ts). Each run uses a throwaway database.

import postgres from 'postgres'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import type { App } from '../src/app.ts'
import { connect, migrateDb } from '../src/db/index.ts'
import { testApp, token } from './helpers.ts'

const ADMIN_URL = process.env.TEST_DATABASE_URL ?? 'postgres://zeima:zeima@localhost:5432/postgres'
const NAME = `zeima_sync_${process.pid}_${Date.now()}`
const admin = postgres(ADMIN_URL, { max: 1, connect_timeout: 2, onnotice: () => {} })
const reachable = await admin`select 1`.then(() => true, () => false)

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const ADA = id(1), BOB = id(2)
const WS = id(100), DEVICE = id(900)
const P1 = id(201), P2 = id(202)
const hlc = (wall: number, counter = 0, node = 'a') => `${String(wall).padStart(15, '0')}-${String(counter).padStart(6, '0')}-${node}`

describe.skipIf(!reachable)('sync API', () => {
  let app: App
  let close: () => Promise<unknown>

  beforeAll(async () => {
    await admin.unsafe(`create database ${NAME}`)
    const url = new URL(ADMIN_URL)
    url.pathname = `/${NAME}`
    const conn = connect(url.toString(), { onnotice: () => {} })
    close = () => conn.client.end()
    await migrateDb(conn.db)
    app = await testApp({ db: conn.db })
  })

  afterAll(async () => {
    await app?.close()
    await close?.()
    await admin.unsafe(`drop database if exists ${NAME} with (force)`).catch(() => {})
    await admin.end()
  })

  const as = async (sub: string) => ({ authorization: `Bearer ${await token({ sub })}` })
  const push = async (sub: string, changes: unknown[], workspaceId = WS) =>
    app.inject({ method: 'POST', url: '/v1/sync/push', headers: await as(sub), payload: { workspaceId, deviceId: DEVICE, changes } })
  const pull = async (sub: string, cursor = 0, limit?: number) =>
    app.inject({ url: `/v1/sync/pull?cursor=${cursor}${limit ? `&limit=${limit}` : ''}`, headers: await as(sub) })
  const change = (rowId: string, h: string, fields: Record<string, unknown>, table = 'project') => ({ table, rowId, op: 'upsert', hlc: h, fields })

  it('requires sign-in', async () => {
    expect((await app.inject({ method: 'POST', url: '/v1/workspaces', payload: { id: WS, name: 'x' } })).statusCode).toBe(401)
    expect((await app.inject('/v1/sync/pull')).statusCode).toBe(401)
  })

  it('creates the personal workspace once, for its owner only', async () => {
    const create = async (sub: string) =>
      app.inject({ method: 'POST', url: '/v1/workspaces', headers: await as(sub), payload: { id: WS, name: 'Ada' } })
    const first = await create(ADA)
    expect(first.statusCode).toBe(200)
    expect(first.json()).toEqual({ id: WS, kind: 'personal', name: 'Ada', orgId: null })
    expect((await create(ADA)).statusCode).toBe(200)
    // Bob cannot see Ada's workspace, so the id looks free to him; the insert is refused.
    expect((await create(BOB)).statusCode).toBe(409)
  })

  it('lists the caller\'s workspaces and no one else\'s', async () => {
    const list = async (sub: string) => (await app.inject({ url: '/v1/workspaces', headers: await as(sub) })).json()
    expect(await list(ADA)).toEqual([{ id: WS, kind: 'personal', name: 'Ada', orgId: null }])
    expect(await list(BOB)).toEqual([])
  })

  it('merges pushed changes and returns them from pull', async () => {
    const res = await push(ADA, [change(P1, hlc(10), { name: 'Alpha', color: '#111' }), change(P2, hlc(11), { name: 'Beta' })])
    expect(res.statusCode).toBe(200)
    expect(res.json().results).toEqual([{ status: 'applied' }, { status: 'applied' }])

    const pulled = (await pull(ADA)).json()
    expect(pulled.rows).toHaveLength(2)
    expect(pulled.more).toBe(false)
    expect(pulled.rows[0]).toEqual({
      workspaceId: WS,
      table: 'project',
      rowId: P1,
      fields: { name: { v: 'Alpha', h: hlc(10) }, color: { v: '#111', h: hlc(10) } },
      deleted: null,
    })
    expect((await pull(ADA, pulled.cursor)).json().rows).toEqual([])
  })

  it('merges field by field: newer wins, older is ignored', async () => {
    const newer = await push(ADA, [change(P1, hlc(20), { name: 'Alpha 2' })])
    expect(newer.json().results).toEqual([{ status: 'applied' }])
    const older = await push(ADA, [change(P1, hlc(5), { name: 'stale', color: '#222' })])
    // color is new information from an older write: it fills nothing because color has hlc 10 > 5.
    expect(older.json().results).toEqual([{ status: 'ignored' }])
    const row = (await pull(ADA)).json().rows.find((r: { rowId: string }) => r.rowId === P1)
    expect(row.fields.name).toEqual({ v: 'Alpha 2', h: hlc(20) })
    expect(row.fields.color).toEqual({ v: '#111', h: hlc(10) })
  })

  it('records deletes as tombstones', async () => {
    const res = await push(ADA, [{ table: 'project', rowId: P2, op: 'delete', hlc: hlc(30) }])
    expect(res.json().results).toEqual([{ status: 'applied' }])
    const row = (await pull(ADA)).json().rows.find((r: { rowId: string }) => r.rowId === P2)
    expect(row.deleted).toBe(hlc(30))
  })

  it('pages with the cursor', async () => {
    const first = (await pull(ADA, 0, 1)).json()
    expect(first.rows).toHaveLength(1)
    expect(first.more).toBe(true)
    const rest = (await pull(ADA, first.cursor, 10)).json()
    expect(rest.rows).toHaveLength(1)
    expect(rest.more).toBe(false)
  })

  it('keeps one user\'s data from another', async () => {
    expect((await push(BOB, [change(P1, hlc(40), { name: 'Bob was here' })])).statusCode).toBe(404)
    expect((await pull(BOB)).json().rows).toEqual([])
  })

  it('accepts a change exactly as the desktop app sends it', async () => {
    // Printed by crates/zeima-core/tests/sync_outbox.rs; keep the two in step.
    const fromDesktop = {
      table: 'project',
      rowId: '01a1d53f-15d7-7199-8c53-14577ace0546',
      op: 'upsert',
      hlc: '001700000000000-000007-01a1d53f-15d5-7658-8fe8-6e2a64959d2d',
      fields: {
        alias: null, billableDefault: false, budgetMinutes: null, budgetPeriod: 'total', budgetWarnPct: 80, client: null,
        color: '#3b82f6', costCenterId: null, createdAt: 1700000000000, name: 'Alpha', parentId: null, status: 'active',
        updatedAt: 1700000000000, workspaceId: '01a1d53f-15d6-75b1-aa41-3a28f650a560',
      },
    }
    const res = await push(ADA, [fromDesktop])
    expect(res.statusCode).toBe(200)
    expect(res.json().results).toEqual([{ status: 'applied' }])
  })

  it('tells other devices when changes were pushed, not the pusher', async () => {
    const other = id(901)
    const headers = await as(ADA)
    const messages: string[] = []
    const mine: string[] = []
    // Listen from the start: the server greets as soon as the socket opens.
    const waiting = await app.injectWS(`/v1/sync/events?deviceId=${other}`, { headers }, { onInit: (ws) => ws.on('message', (m) => messages.push(m.toString())) })
    const pusher = await app.injectWS(`/v1/sync/events?deviceId=${DEVICE}`, { headers }, { onInit: (ws) => ws.on('message', (m) => mine.push(m.toString())) })
    await new Promise((r) => setTimeout(r, 50))
    expect(messages).toEqual([JSON.stringify({ type: 'ready' })])

    expect((await push(ADA, [change(id(210), hlc(60), { name: 'Gamma' })])).statusCode).toBe(200)
    await new Promise((r) => setTimeout(r, 50))
    expect(messages).toEqual([JSON.stringify({ type: 'ready' }), JSON.stringify({ type: 'changes' })])
    expect(mine).toEqual([JSON.stringify({ type: 'ready' })])

    // A push that changes nothing says nothing.
    await push(ADA, [change(id(210), hlc(1), { name: 'old' })])
    await new Promise((r) => setTimeout(r, 50))
    expect(messages).toHaveLength(2)
    waiting.terminate()
    pusher.terminate()
  })

  it('does not tell other users', async () => {
    const messages: string[] = []
    const bob = await app.injectWS('/v1/sync/events', { headers: await as(BOB) }, { onInit: (ws) => ws.on('message', (m) => messages.push(m.toString())) })
    await push(ADA, [change(id(211), hlc(70), { name: 'Delta' })])
    await new Promise((r) => setTimeout(r, 50))
    expect(messages).toEqual([JSON.stringify({ type: 'ready' })])
    bob.terminate()
  })

  it('closes the socket when the token expires', async () => {
    const short = await token({ sub: ADA }, { expiresIn: '2s' })
    const ws = await app.injectWS('/v1/sync/events', { headers: { authorization: `Bearer ${short}` } })
    const code = await new Promise<number>((resolve) => ws.on('close', (c) => resolve(c)))
    expect(code).toBe(4401)
  })

  it('refuses a socket without a token', async () => {
    await expect(app.injectWS('/v1/sync/events')).rejects.toThrow()
  })

  it('validates the request', async () => {
    expect((await push(ADA, [{ table: 'nope', rowId: P1, op: 'upsert', hlc: hlc(1), fields: {} }])).statusCode).toBe(400)
    expect((await push(ADA, [change('not-a-uuid', hlc(1), {})])).statusCode).toBe(400)
    expect((await push(ADA, [change(P1, 'bad-clock', {})])).statusCode).toBe(400)
  })
})
