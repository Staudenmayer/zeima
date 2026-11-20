// Rooms meetings are held in: when each is booked (times only), and whether a
// meeting may take a room. A room is booked by the meetings that name it, also
// through their repeats; two may not overlap, and a meeting called off frees it.
// Mirrors the check of the desktop app (crates/zeima-core/src/repo/meetings.rs),
// which only knows the meetings its person was asked to.

import { and, eq, isNull, sql } from 'drizzle-orm'
import { type Db, schema } from '../db/index.ts'
import { occurrences } from './occurrences.ts'

type Fields = Record<string, unknown>
const plain = (x: unknown): unknown => (x && typeof x === 'object' && 'v' in x ? (x as { v: unknown }).v : x)

export interface Booking {
  from: number
  to: number
}

/** How far ahead a booking is checked (repeating meetings). */
export const ROOM_HORIZON_MS = 2 * 365 * 86_400_000

interface Held {
  id: string
  roomId: string
  startAt: number
  endAt: number
  tz: string
  rrule: string | null
}

/**
 * The meetings that hold a room (of `roomId` only, when given). The database narrows to live meetings that name a room;
 * the checks below stay as the exact rule, so the narrowing may only ever be wider than it.
 */
async function heldRooms(db: Db, workspaceId: string, roomId?: string): Promise<Held[]> {
  const named = roomId === undefined ? sql`${schema.syncRow.fields} ? 'roomId'` : sql`(${schema.syncRow.fields} -> 'roomId' ->> 'v' = ${roomId} or ${schema.syncRow.fields} ->> 'roomId' = ${roomId})`
  const rows = await db
    .select({ rowId: schema.syncRow.rowId, fields: schema.syncRow.fields, deleted: schema.syncRow.deleted, table: schema.syncRow.tableName })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspaceId), eq(schema.syncRow.tableName, 'meeting'), isNull(schema.syncRow.deleted), named))
  const out: Held[] = []
  for (const r of rows) {
    if (r.table !== 'meeting' || r.deleted !== null) continue
    const f = r.fields as Fields
    const room = plain(f.roomId)
    if (typeof room !== 'string' || room === '' || plain(f.cancelled) === true) continue
    out.push({
      id: r.rowId,
      roomId: room,
      startAt: Number(plain(f.startAt)),
      endAt: Number(plain(f.endAt)),
      tz: String(plain(f.tz) ?? 'UTC'),
      rrule: (plain(f.rrule) as string | null) ?? null,
    })
  }
  return out
}

/** When each room is booked within `[from, to)`: the times, never what for. */
export async function roomBookings(db: Db, workspaceId: string, from: number, to: number, exclude?: string): Promise<Record<string, Booking[]>> {
  const out: Record<string, Booking[]> = {}
  for (const m of await heldRooms(db, workspaceId)) {
    if (m.id === exclude) continue
    if (!m.rrule && (m.startAt >= to || m.startAt + Math.max(0, m.endAt - m.startAt) <= from)) continue
    const list = out[m.roomId] ?? []
    out[m.roomId] = list
    for (const [s, e] of occurrences(m.startAt, m.endAt, m.tz, m.rrule, from, to)) list.push({ from: s, to: e })
  }
  for (const [room, list] of Object.entries(out)) {
    if (list.length === 0) delete out[room]
    else list.sort((a, b) => a.from - b.from)
  }
  return out
}

/** Whether the meeting (as these fields say) overlaps another one in its room. */
export async function roomIsTaken(db: Db, workspaceId: string, meetingId: string, merged: Fields): Promise<boolean> {
  const room = plain(merged.roomId)
  if (typeof room !== 'string' || room === '' || plain(merged.cancelled) === true) return false
  const startAt = Number(plain(merged.startAt))
  const endAt = Number(plain(merged.endAt))
  if (!Number.isFinite(startAt) || !Number.isFinite(endAt)) return false
  const horizon = startAt + ROOM_HORIZON_MS
  const mine = occurrences(startAt, endAt, String(plain(merged.tz) ?? 'UTC'), (plain(merged.rrule) as string | null) ?? null, startAt, horizon)
  if (mine.length === 0) return false
  // Only what falls inside the span of this meeting's own times can collide with it.
  const spanFrom = mine[0]![0]
  const spanTo = mine[mine.length - 1]![1]
  for (const other of await heldRooms(db, workspaceId, room)) {
    if (other.roomId !== room || other.id === meetingId) continue
    if (!other.rrule && (other.startAt >= spanTo || other.startAt + Math.max(0, other.endAt - other.startAt) <= spanFrom)) continue
    if (anyOverlap(mine, occurrences(other.startAt, other.endAt, other.tz, other.rrule, spanFrom, spanTo))) return true
  }
  return false
}

/** Whether any interval of `a` overlaps one of `b`; both are sorted by start with equal lengths each, so a sweep of O(a + b) is enough. */
export function anyOverlap(a: readonly (readonly [number, number])[], b: readonly (readonly [number, number])[]): boolean {
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    const [s, e] = a[i]!
    const [c, d] = b[j]!
    if (e <= c) i++
    else if (d <= s) j++
    else return true
  }
  return false
}
