// Database access. `withUser` is how request handlers reach the database:
// it runs a transaction as the restricted `zeima_app` role with the caller's
// identity set, so row-level security (drizzle/0001_rls.sql) applies.
// Anything that uses `db` directly runs as the owner and sees everything:
// migrations, the Keycloak mirror, background jobs.

import { sql } from 'drizzle-orm'
import { drizzle, type PostgresJsDatabase } from 'drizzle-orm/postgres-js'
import { migrate } from 'drizzle-orm/postgres-js/migrator'
import postgres from 'postgres'
import type { AuthUser } from '../plugins/auth.ts'
import * as schema from './schema.ts'

export { schema }
export type Db = PostgresJsDatabase<typeof schema>

export const MIGRATIONS = new URL('../../drizzle', import.meta.url).pathname

export function connect(url: string, options: postgres.Options<Record<string, postgres.PostgresType>> = {}) {
  const client = postgres(url, { max: 10, ...options })
  return { client, db: drizzle(client, { schema }) as Db }
}

/** Arbitrary key of the advisory lock that lets only one process migrate a database at a time. */
const MIGRATION_LOCK = 7_240_001

/**
 * Brings the schema up to date. Several API replicas may start together, so
 * one process migrates while the others wait on a database lock (held by a
 * transaction on one pool connection; the migration uses another, so the pool
 * needs at least two).
 */
export async function migrateDb(db: Db) {
  await db.transaction(async (tx) => {
    await tx.execute(sql`select pg_advisory_xact_lock(${MIGRATION_LOCK})`)
    await migrate(db, { migrationsFolder: MIGRATIONS })
  })
}

export interface Caller {
  userId: string
  orgIds: readonly string[]
}

export function callerOf(user: AuthUser): Caller {
  return { userId: user.id, orgIds: Object.keys(user.orgs) }
}

/** Runs `fn` in a transaction limited to what `caller` may reach. */
export function withUser<T>(db: Db, caller: Caller, fn: (tx: Db) => Promise<T>): Promise<T> {
  return db.transaction(async (tx) => {
    // set_config(..., true) is local to the transaction, like SET LOCAL.
    await tx.execute(sql`select set_config('app.user_id', ${caller.userId}, true),
                                set_config('app.org_ids', ${caller.orgIds.join(',')}, true)`)
    await tx.execute(sql`set local role zeima_app`)
    return fn(tx as unknown as Db)
  })
}
