// What every synced table's fields may hold, so one account cannot fill a shared workspace with values of the wrong
// kind (a string where a time belongs, a megabyte of text in a note). The types come from the app's own models
// (`schema.generated.json`, made by `node scripts/sync-schema.mjs` from the generated bindings; a test keeps it
// current). Names the table does not know pass with generic limits, so a newer app can add a field before the server
// learns it; known names must have their type.

import schema from '../modules/sync/schema.generated.json' with { type: 'json' }

type FieldType = { type: 'string' | 'number' | 'boolean' | 'strings' | 'json'; nullable: boolean }
const SCHEMA = schema as Record<string, Record<string, FieldType>>

/** Fields that travel together as one object (mirrors GROUPS in crates/zeima-core/src/repo/sync_groups.rs). */
export const GROUPS: readonly (readonly [table: string, name: string, members: readonly string[]])[] = [
  ['jurisdiction_selection', 'selection', ['profileId', 'region']],
  ['absence', 'span', ['startDate', 'endDate', 'halfDay']],
  ['time_entry', 'span', ['startAt', 'endAt', 'tz']],
  ['pause', 'span', ['startAt', 'endAt']],
]

/** The plain value of a field: a stored one is `{v, h}`, a pushed one is the value. */
const plain = (x: unknown): unknown => (x && typeof x === 'object' && 'v' in x && 'h' in x ? (x as { v: unknown }).v : x)

/** The fields with their group members spread out (`span` → `startAt`, `endAt`, `tz`), values plain. */
export function flatten(table: string, fields: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [name, value] of Object.entries(fields)) out[name] = plain(value)
  for (const [t, name, members] of GROUPS) {
    const group = t === table ? out[name] : undefined
    if (group && typeof group === 'object' && !Array.isArray(group)) {
      for (const m of members) if (m in (group as object)) out[m] = (group as Record<string, unknown>)[m]
    }
  }
  return out
}

export const MAX_TEXT = 10_000
/** Only these hold a document: a plan of seven days, the auto-pause settings, a rule. */
export const MAX_DOCUMENT = 64 * 1024

function depth(value: unknown, level = 0): number {
  if (value === null || typeof value !== 'object') return level
  return Math.max(level + 1, ...Object.values(value).map((v) => depth(v, level + 1)))
}

/** Whether `value` has the type the table gives `name`. */
export function valueMatches(table: string, name: string, value: unknown): boolean {
  const known = SCHEMA[table]?.[name]
  if (value === null) return known ? known.nullable : true
  if (!known) {
    // Not known to this server: plain values within limits.
    if (typeof value === 'string') return value.length <= MAX_TEXT
    if (typeof value === 'number') return Number.isFinite(value)
    if (typeof value === 'boolean') return true
    return depth(value) <= 4
  }
  switch (known.type) {
    case 'number':
      return typeof value === 'number' && Number.isFinite(value)
    case 'boolean':
      return typeof value === 'boolean'
    case 'string':
      return typeof value === 'string' && value.length <= MAX_TEXT
    case 'strings':
      return Array.isArray(value) && value.length <= 500 && value.every((x) => typeof x === 'string' && x.length <= 64)
    case 'json':
      return typeof value === 'object' && depth(value) <= 4
  }
}

/** Whether every field of a change (and every member of a group field) has the type of its table. */
export function typesAcceptable(table: string, fields: Record<string, unknown>): boolean {
  for (const [name, value] of Object.entries(fields)) {
    const group = GROUPS.find(([t, g]) => t === table && g === name)
    if (group) {
      if (value === null) continue
      if (typeof value !== 'object' || Array.isArray(value)) return false
      const members = Object.entries(value as Record<string, unknown>)
      // Only the members of the group, each with its type.
      if (members.some(([m, v]) => !group[2].includes(m) || !valueMatches(table, m, v))) return false
      continue
    }
    if (!valueMatches(table, name, value)) return false
  }
  return true
}
