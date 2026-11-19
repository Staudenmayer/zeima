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

/** The pool of restricted logins (`zeima_app`) behind a database handle, when there is one. */
const tenantPools = new WeakMap<object, Db>()

/**
 * Opens the database. With `tenantUrl` (a login as `zeima_app`, see `enableTenantLogin`) the requests made through
 * `withUser` use a second pool that connects as that restricted role: they cannot reach anything but what row-level
 * security lets them, and cannot step out of it (a session that merely `SET ROLE`s down from the owner can always
 * `RESET ROLE` up again, and the owner is a superuser in the usual setups).
 */
export function connect(url: string, options: postgres.Options<Record<string, postgres.PostgresType>> = {}, tenantUrl?: string) {
  const client = postgres(url, { max: 10, ...options })
  const db = drizzle(client, { schema }) as Db
  if (!tenantUrl) return { client, db, tenantClient: null }
  const tenantClient = postgres(tenantUrl, { max: 10, ...options })
  tenantPools.set(db, drizzle(tenantClient, { schema }) as Db)
  return { client, db, tenantClient }
}

/** Passwords the API accepts for the restricted login: plain characters only, since the role is set up with DDL. */
export const TENANT_PASSWORD = /^[A-Za-z0-9_.~-]{16,128}$/

/** Lets the restricted role log in (run as the owner, after the migrations). */
export async function enableTenantLogin(db: Db, password: string): Promise<void> {
  if (!TENANT_PASSWORD.test(password)) throw new Error('the password of the restricted database login must be 16 to 128 letters, digits and _ . ~ -')
  await db.execute(sql.raw(`alter role zeima_app with login password '${password}'`))
}

/** The connection string of the restricted login, from the owner's. */
export function tenantUrlFor(databaseUrl: string, password: string): string {
  const url = new URL(databaseUrl)
  url.username = 'zeima_app'
  url.password = password
  return url.toString()
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
  const tenant = tenantPools.get(db as object)
  return (tenant ?? db).transaction(async (tx) => {
    // set_config(..., true) is local to the transaction, like SET LOCAL.
    await tx.execute(sql`select set_config('app.user_id', ${caller.userId}, true),
                                set_config('app.org_ids', ${caller.orgIds.join(',')}, true)`)
    // On the restricted pool the connection already is `zeima_app`; otherwise step down from the owner.
    if (!tenant) await tx.execute(sql`set local role zeima_app`)
    return fn(tx as unknown as Db)
  })
}
