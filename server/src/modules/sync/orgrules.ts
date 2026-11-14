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

