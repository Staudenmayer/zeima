// Advisory locks shared by several modules.

import { sql } from 'drizzle-orm'
import type { Db } from './index.ts'

/** Advisory lock that keeps the commit order of pushes equal to the order of their sequence numbers. */
export const PUSH_ORDER_LOCK = 7_240_002

/**
 * Takes the push lock for the rest of the transaction. Pushes hold it while they run, and whoever removes a member from
 * an organization takes it before deleting the membership and the person's data, so a push either completes first or
 * sees the membership gone. `tx` must be a transaction, not the pool (checked at run time, since callers hold it typed
 * as `Db`): setting, taking and resetting the timeout must all run on that one connection. `timeoutMs` undefined or 0 means "no timeout set" (wait as the session
 * does); a negative or non-finite value is rejected.
 */
export async function lockPushOrder(tx: Pick<Db, 'execute'>, options: { timeoutMs?: number } = {}): Promise<void> {
  // Drizzle's transaction object has `rollback`; the pool does not.
  if (typeof (tx as { rollback?: unknown }).rollback !== 'function') throw new TypeError('lockPushOrder: needs a transaction, not the pool')
  const ms = options.timeoutMs
  if (ms !== undefined && (!Number.isFinite(ms) || ms < 0)) throw new RangeError('lockPushOrder: timeoutMs must be a non-negative number')
  if (!ms || Math.floor(ms) < 1) {
    await tx.execute(sql`select pg_advisory_xact_lock(${PUSH_ORDER_LOCK})`)
    return
  }
  // Wait at most `timeoutMs` (error 55P03 after that); the previous limit (the session's or the role's) comes back
  // afterwards so the one used here does not reach later statements.
  const rows = (await tx.execute(sql`select current_setting('lock_timeout') as v`)) as unknown as { v: string }[]
  const previous = rows[0]?.v ?? '0'
  await tx.execute(sql`select set_config('lock_timeout', ${`${Math.floor(ms)}ms`}, true)`)
  await tx.execute(sql`select pg_advisory_xact_lock(${PUSH_ORDER_LOCK})`)
  await tx.execute(sql`select set_config('lock_timeout', ${previous}, true)`)
}

/** Whether `err` (possibly wrapped by the driver layers) is PostgreSQL's "lock timeout" (55P03). */
export function isLockTimeout(err: unknown): boolean {
  for (let e = err as { code?: unknown; cause?: unknown } | undefined, i = 0; e && i < 5; e = e.cause as typeof e, i++) {
    if (e.code === '55P03') return true
  }
  return false
}
