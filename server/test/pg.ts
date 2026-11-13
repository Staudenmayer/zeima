// A throwaway PostgreSQL database for a test file (see db.test.ts).

import postgres from 'postgres'
import { connect, type Db, migrateDb } from '../src/db/index.ts'

const ADMIN_URL = process.env.TEST_DATABASE_URL ?? 'postgres://zeima:zeima@localhost:5432/postgres'

export async function reachable() {
  const admin = postgres(ADMIN_URL, { max: 1, connect_timeout: 2, onnotice: () => {} })
  const ok = await admin`select 1`.then(() => true, () => false)
  await admin.end()
  return ok
}

export async function createTestDatabase(): Promise<{ db: Db; drop: () => Promise<void> }> {
  const name = `zeima_test_${process.pid}_${Date.now()}_${Math.floor(Math.random() * 1e6)}`
  const admin = postgres(ADMIN_URL, { max: 1, onnotice: () => {} })
  await admin.unsafe(`create database ${name}`)
  const url = new URL(ADMIN_URL)
  url.pathname = `/${name}`
  const conn = connect(url.toString(), { onnotice: () => {} })
  await migrateDb(conn.db)
  return {
    db: conn.db,
    drop: async () => {
      await conn.client.end()
      await admin.unsafe(`drop database if exists ${name} with (force)`).catch(() => {})
      await admin.end()
    },
  }
}
