// Regression tests for the fifth penetration test (PT5-04).
import postgres from 'postgres'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import type { App } from '../src/app.ts'
import { connect, migrateDb } from '../src/db/index.ts'
import { MAX_PERSONAL_WORKSPACES } from '../src/modules/sync/routes.ts'
import { testApp, token } from './helpers.ts'

const ADMIN_URL = process.env.TEST_DATABASE_URL ?? 'postgres://zeima:zeima@localhost:5432/postgres'
const NAME = `zeima_pt5_${process.pid}_${Date.now()}`
const admin = postgres(ADMIN_URL, { max: 1, connect_timeout: 2, onnotice: () => {} })
const reachable = await admin`select 1`.then(() => true, () => false)
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`

describe.skipIf(!reachable)('PT5-04 personal workspaces', () => {
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

  it('an account cannot create workspaces without limit', async () => {
    const headers = { authorization: `Bearer ${await token({ sub: id(1) })}` }
    const codes: number[] = []
    for (let i = 0; i < MAX_PERSONAL_WORKSPACES + 2; i++) {
      codes.push((await app.inject({ method: 'POST', url: '/v1/workspaces', headers, payload: { id: id(100 + i), name: 'x' } })).statusCode)
    }
    expect(codes.slice(0, MAX_PERSONAL_WORKSPACES).every((c) => c === 200)).toBe(true)
    expect(codes.slice(MAX_PERSONAL_WORKSPACES)).toEqual([409, 409])
    // The existing ones stay reachable (idempotent).
    expect((await app.inject({ method: 'POST', url: '/v1/workspaces', headers, payload: { id: id(100), name: 'x' } })).statusCode).toBe(200)
  })
})
