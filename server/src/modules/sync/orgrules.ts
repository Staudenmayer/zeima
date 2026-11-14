// Who may write and read which rows of an organization's shared workspace
// (SPEC §5.9, permissions.yaml). Personal workspaces have no such rules.
//
//   shared   projects, tags, cost centers and the org's policies: everyone
//            reads, changing them needs the matching `*.write` right
//   own      time entries, pauses, absences and a person's own schedule and
//            settings: the owner (`userId` field) writes; others read as far
//            as `entry.read` and friends reach; managers may edit locked
//            entries of their people
//   never    display settings are per device and stay out of org workspaces

import { and, eq, inArray } from 'drizzle-orm'
import { type Db, schema } from '../../db/index.ts'
import { type Actor, can, scopeOf } from '../../plugins/authz.ts'
import type { Change, Outcome, RowState } from '../../sync/merge.ts'

type Kind = 'shared' | 'own' | 'never'

/** The right a change to a shared table needs. */
const SHARED: Record<string, string> = {
  project: 'project.write',
  tag: 'tag.write',
  cost_center: 'cost_center.write',
  cost_center_assignment: 'cost_center.write',
  activity: 'policy.write',
  absence_type: 'policy.write',
  custom_holiday: 'policy.write',
}

/** The right that lets someone see other people's rows of an own table. */
const READ: Record<string, string> = {
  time_entry: 'entry.read',
  pause: 'entry.read',
  absence: 'absence.read',
  balance_adjustment: 'member.read',
  work_schedule: 'member.read',
  work_settings: 'member.read',
  timesheet_submission: 'entry.approve',
}

const OWN = new Set([...Object.keys(READ), 'jurisdiction_selection'])

/** A field's value, from a stored field (`{v, h}`) or a plain one. */
const plain = (x: unknown): unknown => (x && typeof x === 'object' && 'v' in x ? (x as { v: unknown }).v : x)

export function kindOf(table: string, fields: Record<string, unknown>): Kind {
  if (table === 'display_settings') return 'never'
  if (table === 'jurisdiction_selection') {
    // No user: the organization's default; with a user: that person's choice.
    return plain(fields.userId) ? 'own' : 'shared'
  }
  if (table in SHARED) return 'shared'
  return OWN.has(table) ? 'own' : 'never'
}

const ownerOf = (state: RowState | null, change: Change): string | null => {
  const fromState = state?.fields.userId?.v
  if (typeof fromState === 'string') return fromState
  const fromChange = change.fields?.userId
  return typeof fromChange === 'string' ? fromChange : null
}

/** Whether `ownerId` is in a department the actor is in. */
export async function sharesGroup(db: Db, orgId: string, actorGroups: readonly string[], ownerId: string): Promise<boolean> {
  if (actorGroups.length === 0) return false
  const rows = await db
    .select({ userId: schema.kcGroupMember.userId })
    .from(schema.kcGroupMember)
    .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
    .where(and(eq(schema.kcGroup.orgId, orgId), eq(schema.kcGroupMember.userId, ownerId), inArray(schema.kcGroup.path, [...actorGroups])))
    .limit(1)
  return rows.length > 0
}

const DECISION_FIELDS = ['decidedBy', 'decidedAt', 'decisionNote']

/**
 * A timesheet submission: its owner hands it in (and takes it back while it
 * waits, or hands in a rejected one again); someone with `entry.approve` over
 * the owner decides a waiting one, and may touch nothing else.
 */
async function checkSubmission(
  db: Db,
  actor: Actor,
  orgId: string,
  change: Change,
  current: RowState | null,
  owner: string,
): Promise<Outcome | null> {
  const forbidden: Outcome = { status: 'rejected', reason: 'forbidden' }
  const status = plain(current?.fields.status)
  if (owner === actor.user.id) {
    if (current && status !== 'submitted' && status !== 'rejected') return forbidden // decided: closed
    if (change.op === 'delete') return status === 'submitted' ? null : forbidden
    if (change.fields?.status !== undefined && change.fields.status !== 'submitted') return { status: 'rejected', reason: 'forbidden_field' }
    if (!current && change.fields?.status !== 'submitted') return forbidden
    // Handing in again clears the decision; anything else about it is not theirs to write.
    for (const f of DECISION_FIELDS) {
      const v = change.fields?.[f]
      if (v !== undefined && v !== null && v !== '') return { status: 'rejected', reason: 'forbidden_field' }
    }
    return null
  }
  // Someone else's: decide a waiting one, or reopen an approved one (it goes back
  // to the owner as rejected); nothing more.
  if (!current || change.op === 'delete') return forbidden
  const reopening = status === 'approved' && change.fields?.status === 'rejected'
  if (status !== 'submitted' && !reopening) return forbidden
  const scope = scopeOf(actor, 'entry.approve')
  const covered = scope === 'org' || (scope === 'group' && (await sharesGroup(db, orgId, actor.user.orgs[orgId]?.groups ?? [], owner)))
  if (!covered) return forbidden
  const fields = change.fields ?? {}
  const decision = fields.status
  if (decision !== 'approved' && decision !== 'rejected') return { status: 'rejected', reason: 'forbidden_field' }
  if (Object.keys(fields).some((f) => f !== 'status' && !DECISION_FIELDS.includes(f))) return { status: 'rejected', reason: 'forbidden_field' }
  if (fields.decidedBy !== undefined && fields.decidedBy !== actor.user.id) return { status: 'rejected', reason: 'forbidden_field' }
  return null
}

/**
 * An absence in an organization is a request. Its owner writes it (always as
 * `requested`, so changing a decided one asks again); someone with
 * `absence.approve` over the owner decides a waiting one and changes nothing else.
 */
async function checkAbsence(
  db: Db,
  actor: Actor,
  orgId: string,
  change: Change,
  current: RowState | null,
  owner: string,
): Promise<Outcome | null> {
  const forbidden: Outcome = { status: 'rejected', reason: 'forbidden' }
  const badField: Outcome = { status: 'rejected', reason: 'forbidden_field' }
  const fields = change.fields ?? {}
  if (owner === actor.user.id) {
    if (change.op === 'delete') return null
    if (fields.status !== undefined && fields.status !== 'requested') return badField
    // A new request, or a decided one that is changed, goes back to waiting.
    const decided = current && plain(current.fields.status) !== 'requested'
    const touchesMore = Object.keys(fields).some((f) => !['status', ...DECISION_FIELDS].includes(f))
    if ((!current || (decided && touchesMore)) && fields.status !== 'requested') return badField
    for (const f of DECISION_FIELDS) {
      const v = fields[f]
      if (v !== undefined && v !== null && v !== '') return badField
    }
    return null
  }
  if (!current || change.op === 'delete' || plain(current.fields.status) !== 'requested') return forbidden
  const scope = scopeOf(actor, 'absence.approve')
  const covered = scope === 'org' || (scope === 'group' && (await sharesGroup(db, orgId, actor.user.orgs[orgId]?.groups ?? [], owner)))
  if (!covered) return forbidden
  if (fields.status !== 'approved' && fields.status !== 'rejected') return badField
  if (Object.keys(fields).some((f) => f !== 'status' && !DECISION_FIELDS.includes(f))) return badField
  if (fields.decidedBy !== undefined && fields.decidedBy !== actor.user.id) return badField
  return null
}

/** Null if the change may go on to the merge; otherwise the reason it is refused. */
export async function checkOrgWrite(
  db: Db,
  actor: Actor,
  orgId: string,
  change: Change,
  current: RowState | null,
): Promise<Outcome | null> {
  const merged = { ...(current?.fields ?? {}), ...(change.fields ?? {}) }
  const kind = kindOf(change.table, merged)
  if (kind === 'never') return { status: 'rejected', reason: 'forbidden_table' }
  if (kind === 'shared') {
    const action = SHARED[change.table] ?? 'policy.write'
    if (can(actor, action, { orgId })) return null
    // A project's manager runs it: visibility and members, not who the manager is.
    if (change.table === 'project' && current && plain(current.fields.managerId) === actor.user.id) {
      return change.fields && 'managerId' in change.fields && change.fields.managerId !== actor.user.id
        ? { status: 'rejected', reason: 'forbidden_field' }
        : null
    }
    return { status: 'rejected', reason: 'forbidden' }
  }
  // Own rows: the owner never changes after the row exists.
  const owner = ownerOf(current, change)
  if (!owner) return { status: 'rejected', reason: 'forbidden' }
  if (change.table === 'timesheet_submission') return checkSubmission(db, actor, orgId, change, current, owner)
  if (change.table === 'absence') return checkAbsence(db, actor, orgId, change, current, owner)
  const moved = change.fields?.userId
  if (current && typeof moved === 'string' && moved !== owner) return { status: 'rejected', reason: 'forbidden_field' }
  if (owner === actor.user.id) return null
  // Someone else's row: only managers editing time entries (locking, corrections).
  if (change.table !== 'time_entry') return { status: 'rejected', reason: 'forbidden' }
  const scope = scopeOf(actor, 'entry.write_locked')
  if (scope === 'org') return null
  if (scope === 'group' && (await sharesGroup(db, orgId, actor.user.orgs[orgId]?.groups ?? [], owner))) return null
  return { status: 'rejected', reason: 'forbidden' }
}

/** A members-only project is seen by those who manage all projects, its manager and its members. */
function canSeeProject(user: Actor['user'], orgId: string, fields: Record<string, unknown>): boolean {
  if (can({ user, orgId }, 'project.write', { orgId })) return true
  if (plain(fields.managerId) === user.id) return true
  const members = plain(fields.memberIds)
  return Array.isArray(members) && members.includes(user.id)
}

/**
 * The rows of org workspaces the caller may see: shared rows for everyone,
 * own-table rows for their owner and for roles whose read right reaches them.
 */
export async function visibleToCaller<T extends { workspaceId: string; table: string; fields: Record<string, { v: unknown }> }>(
  db: Db,
  actor: Actor['user'],
  workspaces: Map<string, string>,
  rows: T[],
): Promise<T[]> {
  const sharing = new Map<string, Set<string>>()
  const out: T[] = []
  for (const row of rows) {
    const orgId = workspaces.get(row.workspaceId)
    if (!orgId) {
      out.push(row) // personal workspace
      continue
    }
    const kind = kindOf(row.table, row.fields)
    if (kind === 'never') continue
    if (kind === 'shared') {
      if (row.table === 'project' && plain(row.fields.visibility) === 'members' && !canSeeProject(actor, orgId, row.fields)) continue
      out.push(row)
      continue
    }
    const owner = typeof row.fields.userId?.v === 'string' ? (row.fields.userId.v as string) : null
    if (owner === actor.id) {
      out.push(row)
      continue
    }
    if (!owner) continue
    const scope = scopeOf({ user: actor, orgId }, READ[row.table] ?? 'member.read')
    if (scope === 'org') out.push(row)
    else if (scope === 'group') {
      let set = sharing.get(orgId)
      if (!set) {
        set = new Set()
        const paths = actor.orgs[orgId]?.groups ?? []
        if (paths.length) {
          const members = await db
            .select({ userId: schema.kcGroupMember.userId })
            .from(schema.kcGroupMember)
            .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
            .where(and(eq(schema.kcGroup.orgId, orgId), inArray(schema.kcGroup.path, [...paths])))
          for (const m of members) set.add(m.userId)
        }
        sharing.set(orgId, set)
      }
      if (set.has(owner)) out.push(row)
    }
  }
  return out
}

