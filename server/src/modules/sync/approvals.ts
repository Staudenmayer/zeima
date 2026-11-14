// Approving a timesheet locks the entries of its period (SPEC §5.9). The
// server does it, so it holds whoever has the entries and wherever the
// approver's app is.

import { sql } from 'drizzle-orm'
import type { Db } from '../../db/index.ts'
import type { RowState } from '../../sync/merge.ts'

const OFFSET_PARTS = ['year', 'month', 'day', 'hour', 'minute', 'second'] as const

/** Offset of `tz` from UTC at the instant `ms`, in milliseconds. */
function offsetMs(ms: number, tz: string): number {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: tz,
    hourCycle: 'h23',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).formatToParts(new Date(ms))
  const v = Object.fromEntries(parts.filter((p) => (OFFSET_PARTS as readonly string[]).includes(p.type)).map((p) => [p.type, Number(p.value)]))
  return Date.UTC(v.year!, v.month! - 1, v.day!, v.hour!, v.minute!, v.second!) - Math.floor(ms / 1000) * 1000
}

/** Midnight at the start of `date` (`YYYY-MM-DD`) in `tz`, as unix ms. */
export function startOfDay(date: string, tz: string): number {
  const [y, m, d] = date.split('-').map(Number) as [number, number, number]
  const wall = Date.UTC(y, m - 1, d)
  // The offset at midnight itself; one correction covers a change of offset on that day.
  const first = wall - offsetMs(wall, tz)
  return wall - offsetMs(first, tz)
}

const nextDay = (date: string) => new Date(Date.parse(`${date}T00:00:00Z`) + 86_400_000).toISOString().slice(0, 10)

const value = (state: RowState, field: string) => state.fields[field]?.v

/** Whether this state of a submission is an approved one. */
export const isApproved = (state: RowState | null) => state?.fields.status?.v === 'approved'

/** Locks (or, reopening a period, unlocks) the person's time entries that start in the period; returns how many. */
export async function setPeriodLocked(tx: Db, workspaceId: string, submission: RowState, hlc: string, locked: boolean): Promise<number> {
  const user = value(submission, 'userId')
  const start = value(submission, 'periodStart')
  const end = value(submission, 'periodEnd')
  const tz = value(submission, 'tz')
  if (typeof user !== 'string' || typeof start !== 'string' || typeof end !== 'string' || typeof tz !== 'string') return 0
  let from: number
  let to: number
  try {
    from = startOfDay(start, tz)
    to = startOfDay(nextDay(end), tz)
  } catch {
    return 0 // an unknown zone locks nothing
  }
  // Wins over any earlier write of the flag; a later change by a manager wins again.
  const stamp = `${String(Date.now()).padStart(15, '0')}-000000-server`
  const lockHlc = stamp > hlc ? stamp : hlc
  const result = await tx.execute(sql`
    UPDATE sync_row
       SET fields = jsonb_set(fields, '{locked}', jsonb_build_object('v', ${locked}::boolean, 'h', ${lockHlc}::text), true),
           seq = nextval('sync_seq')
     WHERE workspace_id = ${workspaceId}
       AND table_name = 'time_entry'
       AND deleted IS NULL
       AND fields -> 'userId' ->> 'v' = ${user}
       AND (fields -> 'startAt' ->> 'v')::bigint >= ${from}
       AND (fields -> 'startAt' ->> 'v')::bigint < ${to}
       AND coalesce(fields -> 'locked' ->> 'v', 'false') <> ${String(locked)}`)
  return (result as unknown as { count?: number }).count ?? 0
}
