// Sync merge (SPEC §6): field-level last-write-wins by HLC, plus the rule
// overrides the server enforces. A pure, state-based merge: every field keeps
// the value and the HLC of the write that set it, and the row keeps the HLC of
// its latest delete. Merging takes the newer HLC per field and the newer
// delete, so it is commutative and idempotent.
//
// Mirrors crates/zeima-core/src/sync.rs. Both run the vectors in
// packages/shared-fixtures/sync/merge.json; change one, change the other.
// HLC strings are fixed-width, so comparing them as strings compares the
// timestamps and the node id breaks exact ties.

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }

export interface Versioned {
  v: Json
  h: string
}

export interface RowState {
  fields: Record<string, Versioned>
  /** HLC of the latest delete. */
  deleted: string | null
}

export interface Change {
  table: string
  rowId: string
  op: 'upsert' | 'delete'
  hlc: string
  /** Only the fields that changed. */
  fields?: Record<string, Json>
}

export interface Actor {
  manager: boolean
}

export type Reason = 'locked' | 'forbidden_field' | 'forbidden' | 'forbidden_table' | 'policy_project' | 'policy_tags' | 'room_taken'

export type Outcome = { status: 'applied' } | { status: 'ignored' } | { status: 'rejected'; reason: Reason }

const TIME_ENTRY = 'time_entry'
const LOCKED = 'locked'

export const emptyRow = (): RowState => ({ fields: {}, deleted: null })

/** A row is live unless it was deleted and no field was written after. */
export function isLive(state: RowState): boolean {
  const { deleted } = state
  return deleted === null || Object.values(state.fields).some((f) => f.h > deleted)
}

const isLocked = (state: RowState) => state.fields[LOCKED]?.v === true

/** Locked time entries are changed by managers only, and only managers change the lock. */
function checkRules(state: RowState, change: Change, actor: Actor): Reason | null {
  if (change.table !== TIME_ENTRY || actor.manager) return null
  if (isLocked(state)) return 'locked'
  const next = change.fields?.[LOCKED]
  if (next !== undefined && next !== isLocked(state)) return 'forbidden_field'
  return null
}

/** Merges `change` into `state` (`null`: the row is unknown). Does not modify `state`. */
export function merge(state: RowState | null, change: Change, actor: Actor): { state: RowState; outcome: Outcome } {
  const current = state ?? emptyRow()
  const reason = checkRules(current, change, actor)
  if (reason) return { state: current, outcome: { status: 'rejected', reason } }

  const next: RowState = { fields: { ...current.fields }, deleted: current.deleted }
  let changed = false
  if (change.op === 'delete') {
    if (next.deleted === null || change.hlc > next.deleted) {
      next.deleted = change.hlc
      changed = true
    }
  } else {
    for (const [name, value] of Object.entries(change.fields ?? {})) {
      const existing = next.fields[name]
      if (!existing || change.hlc > existing.h) {
        next.fields[name] = { v: value, h: change.hlc }
        changed = true
      }
    }
  }
  return { state: next, outcome: { status: changed ? 'applied' : 'ignored' } }
}

/** Applies changes in order; a rejected change leaves an unknown row unknown. */
export function mergeAll(state: RowState | null, changes: readonly Change[], actor: Actor) {
  let current = state
  const outcomes: Outcome[] = []
  for (const change of changes) {
    const result = merge(current, change, actor)
    if (result.outcome.status !== 'rejected') current = result.state
    outcomes.push(result.outcome)
  }
  return { state: current, outcomes }
}
