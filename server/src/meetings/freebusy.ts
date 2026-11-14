// When people are busy, from what the server knows: meetings they organize
// or accepted (or have not answered), and absences that were approved.
// Only times are returned, never what they are for.

import { eq } from 'drizzle-orm'
import { type Db, schema } from '../db/index.ts'
import { startOfDay } from '../modules/sync/approvals.ts'
import { occurrences } from './occurrences.ts'

export interface Busy {
  from: number
  to: number
  kind: 'meeting' | 'absence'
}

type Fields = Record<string, { v: unknown; h: string }>
const value = (f: Fields, k: string) => f[k]?.v

const nextDay = (date: string) => new Date(Date.parse(`${date}T00:00:00Z`) + 86_400_000).toISOString().slice(0, 10)

/** Busy times of `userIds` within `[from, to)`; `tz` is the zone absence days are read in. */
export async function freeBusy(db: Db, workspaceId: string, userIds: readonly string[], from: number, to: number, tz: string): Promise<Record<string, Busy[]>> {
  const wanted = new Set(userIds)
  const out: Record<string, Busy[]> = Object.fromEntries(userIds.map((u) => [u, []]))
  const rows = await db
    .select({ table: schema.syncRow.tableName, rowId: schema.syncRow.rowId, fields: schema.syncRow.fields, deleted: schema.syncRow.deleted })
    .from(schema.syncRow)
    .where(eq(schema.syncRow.workspaceId, workspaceId))
  const live = rows.filter((r) => r.deleted === null)
  const declined = new Set<string>() // `${meetingId}|${userId}`
  for (const r of live) {
    if (r.table !== 'meeting_response') continue
    const f = r.fields as Fields
    if (value(f, 'status') === 'declined') declined.add(`${String(value(f, 'meetingId'))}|${String(value(f, 'userId'))}`)
  }
  for (const r of live) {
    const f = r.fields as Fields
    if (r.table === 'meeting' && value(f, 'cancelled') !== true) {
      const organizer = String(value(f, 'userId'))
      const attendees = Array.isArray(value(f, 'attendeeIds')) ? (value(f, 'attendeeIds') as string[]) : []
      const busyFor = [organizer, ...attendees].filter((u) => wanted.has(u) && !declined.has(`${r.rowId}|${u}`))
      if (busyFor.length === 0) continue
      const times = occurrences(Number(value(f, 'startAt')), Number(value(f, 'endAt')), String(value(f, 'tz') ?? 'UTC'), (value(f, 'rrule') as string | null) ?? null, from, to)
      for (const u of busyFor) for (const [s, e] of times) out[u]!.push({ from: s, to: e, kind: 'meeting' })
    } else if (r.table === 'absence' && value(f, 'status') === 'approved') {
      const user = String(value(f, 'userId'))
      if (!wanted.has(user)) continue
      const start = String(value(f, 'startDate'))
      const end = String(value(f, 'endDate'))
      let s: number
      let e: number
      try {
        s = startOfDay(start, tz)
        e = startOfDay(nextDay(end), tz)
      } catch {
        continue
      }
      if (s < to && e > from) out[user]!.push({ from: s, to: e, kind: 'absence' })
    }
  }
  for (const list of Object.values(out)) list.sort((a, b) => a.from - b.from)
  return out
}

