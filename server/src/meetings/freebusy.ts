// When people are busy, from what the server knows: meetings they organize
// or accepted (or have not answered), and absences that were approved.
// Only times are returned, never what they are for.

import { eq } from 'drizzle-orm'
import { type Db, schema } from '../db/index.ts'
import { occurrences } from './occurrences.ts'
import { startOfDay } from './zones.ts'

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
  // What each person answered: to the whole series (`meeting|user`) and to single dates (`meeting|user|start`).
  const answers = new Map<string, unknown>()
  for (const r of live) {
    if (r.table !== 'meeting_response') continue
    const f = r.fields as Fields
    const at = value(f, 'occurrenceAt')
    answers.set(`${String(value(f, 'meetingId'))}|${String(value(f, 'userId'))}${typeof at === 'number' ? `|${at}` : ''}`, value(f, 'status'))
  }
  /** Declined for this date: the date's own answer counts, else the series'. */
  const declinedAt = (meetingId: string, user: string, start: number) => (answers.get(`${meetingId}|${user}|${start}`) ?? answers.get(`${meetingId}|${user}`)) === 'declined'
  for (const r of live) {
    const f = r.fields as Fields
    if (r.table === 'meeting' && value(f, 'cancelled') !== true) {
      const organizer = String(value(f, 'userId'))
      const attendees = Array.isArray(value(f, 'attendeeIds')) ? (value(f, 'attendeeIds') as string[]) : []
      const people = [organizer, ...attendees].filter((u) => wanted.has(u))
      if (people.length === 0) continue
      const times = occurrences(Number(value(f, 'startAt')), Number(value(f, 'endAt')), String(value(f, 'tz') ?? 'UTC'), (value(f, 'rrule') as string | null) ?? null, from, to)
      for (const u of people) for (const [s, e] of times) if (!declinedAt(r.rowId, u, s)) out[u]!.push({ from: s, to: e, kind: 'meeting' })
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

