// Advisory locks shared by several modules.

import { sql } from 'drizzle-orm'
import type { Db } from './index.ts'

/** Advisory lock that keeps the commit order of pushes equal to the order of their sequence numbers. */
export const PUSH_ORDER_LOCK = 7_240_002

/**
 * Takes the push lock for the rest of the transaction. Pushes hold it while they run, and whoever removes a member from
 * an organization takes it before deleting the membership and the person's data, so a push either completes first or
 * sees the membership gone.
 */
export async function lockPushOrder(tx: Pick<Db, 'execute'>): Promise<void> {
  await tx.execute(sql`select pg_advisory_xact_lock(${PUSH_ORDER_LOCK})`)
}
