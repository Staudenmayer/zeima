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

import { and, eq, inArray, sql } from 'drizzle-orm'
import { type Db, schema } from '../../db/index.ts'
import { roomIsTaken } from '../../meetings/rooms.ts'
import { meetingShapeProblem } from '../../meetings/occurrences.ts'
import { type Actor, can, scopeOf } from '../../plugins/authz.ts'
import type { Change, Outcome, RowState } from '../../sync/merge.ts'
import { flatten } from '../../sync/schema.ts'
import { departmentsOf, effective, loadPolicies, orgWorkspaceId, type PolicyRow } from './policy.ts'

type Kind = 'shared' | 'own' | 'never'

/** The right a change to a shared table needs. */
const SHARED: Record<string, string> = {
  project: 'project',
  tag: 'tag',
  cost_center: 'cost_center',
  cost_center_assignment: 'cost_center',
  activity: 'policy',
  absence_type: 'policy',
  org_policy: 'policy',
  office: 'policy',
  room: 'policy',
  custom_holiday: 'policy',
}
/** Changing a shared row takes `<kind>.write`, deleting it `<kind>.delete`. */
const sharedAction = (table: string, op: string) => `${SHARED[table] ?? 'policy'}.${op === 'delete' ? 'delete' : 'write'}`

/** The right that lets someone see a shared table at all (the others are everybody's). */
const SHARED_READ: Record<string, string> = { project: 'project.read', cost_center: 'cost_center.read', cost_center_assignment: 'cost_center.read' }

/** The right that lets someone see other people's rows of an own table. */
const READ: Record<string, string> = {
  time_entry: 'entry.read',
  pause: 'entry.read',
  absence: 'absence.read',
  balance_adjustment: 'member.read',
  work_schedule: 'member.read',
  work_settings: 'member.read',
  work_location: 'member.read',
  meeting: 'member.read',
  meeting_response: 'member.read',
}

/** Own rows everyone in the organization may read (where people work is for planning together). */
const PUBLIC = new Set(['work_location'])

/** Own tables that the person does not write in an organization: see `checkOrgWrite`. */
export const MANAGED = new Set(['balance_adjustment', 'work_schedule', 'work_settings'])

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

/** A finished entry must meet the policy of its owner (a project, tags). */
async function checkEntryPolicy(db: Db, actor: Actor, orgId: string, change: Change, merged: Record<string, unknown>): Promise<Outcome | null> {
  if (change.op === 'delete' || plain(merged.endAt) == null) return null
  const workspace = await orgWorkspaceId(db, orgId)
  if (!workspace) return null
  const policy = effective(await loadPolicies(db, workspace), actor.user.orgs[orgId]?.groups.map((g) => g.slice(`/orgs/${orgId}/groups/`.length)) ?? [])
  if (policy.requireProject && !plain(merged.projectId)) return { status: 'rejected', reason: 'policy_project' }
  const tags = plain(merged.tagIds)
  if (policy.requireTags && !(Array.isArray(tags) && tags.length > 0)) return { status: 'rejected', reason: 'policy_tags' }
  return null
}

/**
 * A pause is part of the time it interrupts: it cannot be added to, or moved into, a locked entry of the same person
 * (that would change the net time of a period a manager has closed) unless the writer may edit locked entries.
 */
/** The locked time entries of a person, looked up once per request (a push has up to 500 changes, one transaction). */
const lockedCache = new WeakMap<object, Map<string, { rowId: string; fields: unknown }[]>>()
async function lockedEntriesOf(db: Db, workspace: string, owner: string) {
  let perTx = lockedCache.get(db)
  if (!perTx) {
    perTx = new Map()
    lockedCache.set(db, perTx)
  }
  const key = `${workspace}|${owner}`
  const hit = perTx.get(key)
  if (hit) return hit
  const rows = await db
    .select({ rowId: schema.syncRow.rowId, fields: schema.syncRow.fields })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspace), eq(schema.syncRow.tableName, 'time_entry'), sql`${schema.syncRow.fields} -> 'userId' ->> 'v' = ${owner}`, sql`${schema.syncRow.fields} -> 'locked' ->> 'v' = 'true'`))
  perTx.set(key, rows)
  return rows
}

async function checkPause(db: Db, actor: Actor, orgId: string, owner: string, merged: Record<string, unknown>): Promise<Outcome | null> {
  if (scopeOf(actor, 'entry.write_locked') === 'org') return null
  const workspace = await orgWorkspaceId(db, orgId)
  if (!workspace) return null
  const start = plain(merged.startAt)
  const end = plain(merged.endAt)
  const resume = plain(merged.resumeEntryId)
  const rows = await lockedEntriesOf(db, workspace, owner)
  for (const r of rows) {
    const f = flatten('time_entry', r.fields as Record<string, unknown>)
    const entryStart = f.startAt
    const entryEnd = f.endAt
    const overlaps =
      typeof start === 'number' && typeof entryStart === 'number' && typeof entryEnd === 'number' && start < entryEnd && (typeof end !== 'number' || end > entryStart)
    if (r.rowId === resume || overlaps) return { status: 'rejected', reason: 'locked' }
  }
  return null
}

/** Fields that point at a row of another table: table → field → the table it points into (`[]`: a list of ids). */
const REFERENCES: Record<string, Record<string, { to: string; list?: boolean }>> = {
  time_entry: { projectId: { to: 'project' }, activityId: { to: 'activity' }, costCenterId: { to: 'cost_center' }, tagIds: { to: 'tag', list: true } },
  pause: { resumeEntryId: { to: 'time_entry' }, followupEntryId: { to: 'time_entry' } },
  meeting: { projectId: { to: 'project' }, activityId: { to: 'activity' }, costCenterId: { to: 'cost_center' }, tagIds: { to: 'tag', list: true }, roomId: { to: 'room' } },
  absence: { typeId: { to: 'absence_type' } },
  cost_center_assignment: { costCenterId: { to: 'cost_center' }, projectId: { to: 'project' } },
}

/**
 * Everything a change points at must be a live row of the right table in this workspace, and a members-only project
 * (or a tag of one) must be one the author may see: nobody books time on, or attaches things to, what they cannot use.
 */
async function checkReferences(db: Db, actor: Actor, orgId: string, change: Change): Promise<Outcome | null> {
  const spec = REFERENCES[change.table]
  if (change.op === 'delete' || !change.fields || !spec) return null
  const wanted: { id: string; to: string }[] = []
  for (const [field, { to, list }] of Object.entries(spec)) {
    if (!(field in change.fields)) continue
    const value = change.fields[field]
    if (value === null || value === undefined) continue
    const ids = list ? (Array.isArray(value) ? value : [null]) : [value]
    for (const id of ids) {
      if (typeof id !== 'string') return { status: 'rejected', reason: 'forbidden_field' }
      wanted.push({ id, to })
    }
  }
  if (wanted.length === 0) return null
  const workspace = await orgWorkspaceId(db, orgId)
  if (!workspace) return { status: 'rejected', reason: 'forbidden_field' }
  const rows = await db
    .select({ rowId: schema.syncRow.rowId, table: schema.syncRow.tableName, fields: schema.syncRow.fields, deleted: schema.syncRow.deleted })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspace), inArray(schema.syncRow.rowId, [...new Set(wanted.map((w) => w.id))])))
  const byId = new Map(rows.map((r) => [r.rowId, r]))
  const maySeeAll = scopeOf(actor, 'entry.write_locked') === 'org'
  const hidden = (fields: Record<string, unknown>) => plain(fields.visibility) === 'members' && !canSeeProject(actor.user, orgId, fields) && !maySeeAll
  for (const { id, to } of wanted) {
    const row = byId.get(id)
    if (!row || row.table !== to) return { status: 'rejected', reason: 'forbidden_field' }
    const live = row.deleted === null || Object.values(row.fields as Record<string, { h: string }>).some((f) => f.h > row.deleted!)
    if (!live) return { status: 'rejected', reason: 'forbidden_field' }
    const fields = row.fields as Record<string, unknown>
    if (to === 'project' && hidden(fields)) return { status: 'rejected', reason: 'forbidden_field' }
    if (to === 'tag') {
      const project = plain(fields.projectId)
      const owner = typeof project === 'string' ? byId.get(project) : undefined
      if (typeof project === 'string') {
        const [p] = owner ? [owner] : await db.select({ fields: schema.syncRow.fields }).from(schema.syncRow).where(and(eq(schema.syncRow.workspaceId, workspace), eq(schema.syncRow.rowId, project)))
        if (p && hidden(p.fields as Record<string, unknown>)) return { status: 'rejected', reason: 'forbidden_field' }
      }
    }
  }
  return null
}

/** A person answers only a meeting they are asked to (and not their own). */
async function checkMeetingResponse(db: Db, orgId: string, me: string, merged: Record<string, unknown>): Promise<Outcome | null> {
  const meetingId = plain(merged.meetingId)
  const workspace = await orgWorkspaceId(db, orgId)
  if (typeof meetingId !== 'string' || !workspace) return { status: 'rejected', reason: 'forbidden' }
  // An answer for one date of a repeating meeting names that date (unix ms); without one it is for the series.
  const date = plain(merged.occurrenceAt)
  if (date !== undefined && date !== null && !Number.isFinite(date)) return { status: 'rejected', reason: 'forbidden' }
  const [row] = await db
    .select({ fields: schema.syncRow.fields })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspace), eq(schema.syncRow.tableName, 'meeting'), eq(schema.syncRow.rowId, meetingId)))
  const fields = (row?.fields ?? {}) as Record<string, unknown>
  const attendees = plain(fields.attendeeIds)
  const asked = Array.isArray(attendees) && attendees.includes(me) && plain(fields.userId) !== me
  return asked ? null : { status: 'rejected', reason: 'forbidden' }
}

/** A meeting must be of a sane shape (times, repeat rule, zone) and invite only members of the organization. */
async function meetingProblem(db: Db, orgId: string, merged: Record<string, unknown>): Promise<string | null> {
  const shape = meetingShapeProblem(plain(merged.startAt), plain(merged.endAt), plain(merged.tz), plain(merged.rrule))
  if (shape) return shape
  const attendees = plain(merged.attendeeIds)
  if (Array.isArray(attendees) && attendees.length > 0) {
    if (attendees.length > 200) return 'too many attendees'
    const ids = [...new Set(attendees.filter((a): a is string => typeof a === 'string'))]
    const found = await db.select({ id: schema.kcOrgMember.userId }).from(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, orgId), inArray(schema.kcOrgMember.userId, ids)))
    if (found.length !== ids.length) return 'attendee is not a member'
  }
  return null
}

const DECISION_FIELDS = ['decidedBy', 'decidedAt', 'decisionNote']

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
    // A decided request is a record (who decided, when): its owner asks again by changing it, but cannot make it vanish.
    if (change.op === 'delete') return current && plain(current.fields.status) !== 'requested' ? forbidden : null
    // The decision fields are the approver's: the owner clears them only by asking again (status `requested`).
    if (DECISION_FIELDS.some((f) => f in fields) && fields.status !== 'requested') return badField
    // A decided absence changes only by a change newer than the decision: the stamp is the client's claim, and a
    // back-dated one would otherwise change the other fields while the decision keeps standing.
    const decisionStamp = current ? (current.fields.status?.h ?? null) : null
    if (current && plain(current.fields.status) !== 'requested' && decisionStamp !== null && change.hlc <= decisionStamp) return forbidden
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
  const merged = flatten(change.table, { ...(current?.fields ?? {}), ...(change.fields ?? {}) })
  // The kind comes from the row as stored: the writer must not be able to turn a shared row into their own (or back)
  // with the very change that edits it.
  if (current && change.fields && 'userId' in change.fields && (change.fields.userId ?? null) !== (plain(current.fields.userId) ?? null)) {
    return { status: 'rejected', reason: 'forbidden_field' }
  }
  const kind = current ? kindOf(change.table, flatten(change.table, current.fields)) : kindOf(change.table, merged)
  if (kind === 'never') return { status: 'rejected', reason: 'forbidden_table' }
  const dangling = await checkReferences(db, actor, orgId, change)
  if (dangling) return dangling
  if (kind === 'shared') {
    const action = sharedAction(change.table, change.op)
    if (can(actor, action, { orgId })) return null
    // A project's manager runs it: visibility and members, not who the manager is.
    if (change.table === 'project' && change.op !== 'delete' && current && plain(current.fields.managerId) === actor.user.id) {
      return change.fields && 'managerId' in change.fields && change.fields.managerId !== actor.user.id
        ? { status: 'rejected', reason: 'forbidden_field' }
        : null
    }
    return { status: 'rejected', reason: 'forbidden' }
  }
  // Own rows: the owner never changes after the row exists.
  const owner = ownerOf(current, change)
  if (!owner) return { status: 'rejected', reason: 'forbidden' }
  if (change.table === 'absence') return checkAbsence(db, actor, orgId, change, current, owner)
  if (change.table === 'meeting_response' && owner === actor.user.id) return checkMeetingResponse(db, orgId, actor.user.id, merged)
  if (change.table === 'meeting' && owner === actor.user.id && change.op !== 'delete') {
    const bad = await meetingProblem(db, orgId, merged)
    if (bad) return { status: 'rejected', reason: 'forbidden_field' }
    const workspace = await orgWorkspaceId(db, orgId)
    if (workspace && (await roomIsTaken(db, workspace, change.rowId, merged))) return { status: 'rejected', reason: 'room_taken' }
  }
  const moved = change.fields?.userId
  if (current && typeof moved === 'string' && moved !== owner) return { status: 'rejected', reason: 'forbidden_field' }
  // The employer's records about a person, not the person's own: their schedule, vacation entitlement and break rules,
  // and overtime corrections. An organization sets them (`member.manage`); in a personal workspace the person does.
  if (MANAGED.has(change.table)) return scopeOf(actor, 'member.manage') === 'org' ? null : { status: 'rejected', reason: 'forbidden' }
  if (owner === actor.user.id) {
    if (change.table === 'pause') return checkPause(db, actor, orgId, owner, merged)
    if (change.table !== 'time_entry') return null
    // A locked entry is frozen for the person it belongs to as well. Only someone who may edit everybody's locked
    // entries (organization-wide) can change their own: a manager of one department does not lift an admin's lock on themselves.
    if (current && plain(current.fields.locked) === true && scopeOf(actor, 'entry.write_locked') !== 'org') return { status: 'rejected', reason: 'locked' }
    return checkEntryPolicy(db, actor, orgId, change, merged)
  }
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
/** Meetings of a workspace by id, as their stored fields. */
async function loadMeetings(db: Db, workspaceId: string): Promise<Map<string, Record<string, { v: unknown; h: string }>>> {
  const rows = await db
    .select({ rowId: schema.syncRow.rowId, fields: schema.syncRow.fields })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspaceId), eq(schema.syncRow.tableName, 'meeting')))
  return new Map(rows.map((r) => [r.rowId, r.fields as Record<string, { v: unknown; h: string }>]))
}

/** Whether the person organizes the meeting or is asked to it. */
const isAsked = (fields: Record<string, { v: unknown }>, userId: string) => {
  const attendees = plain(fields.attendeeIds)
  return plain(fields.userId) === userId || (Array.isArray(attendees) && attendees.includes(userId))
}

const departmentCache = new WeakMap<Map<string, PolicyRow[]>, Map<string, string[]>>()

/** The entry fields a manager does not get, by what the owner's policy shows. */
const HIDDEN_BY_TOTALS = ['note', 'projectId', 'tagIds', 'activityId', 'costCenterId']

/**
 * What a manager sees of someone's time entry: all of it, or, if the owner's
 * policy says so, without the note or with only the times.
 */
async function limited<T extends { workspaceId: string; table: string; fields: Record<string, { v: unknown; h: string }> }>(
  db: Db,
  row: T,
  orgId: string,
  owner: string,
  cache: Map<string, PolicyRow[]>,
): Promise<T> {
  if (row.table !== 'time_entry') return row
  let rows = cache.get(row.workspaceId)
  if (!rows) {
    rows = await loadPolicies(db, row.workspaceId)
    cache.set(row.workspaceId, rows)
  }
  if (rows.length === 0) return row
  // One lookup per person for the whole page of rows, not one per row.
  let known = departmentCache.get(cache)
  if (!known) {
    known = new Map()
    departmentCache.set(cache, known)
  }
  let departments = known.get(owner)
  if (!departments) {
    departments = await departmentsOf(db, orgId, owner)
    known.set(owner, departments)
  }
  const { managerVisibility } = effective(rows, departments)
  if (managerVisibility === 'full') return row
  const hide = managerVisibility === 'hide_notes' ? ['note'] : HIDDEN_BY_TOTALS
  const fields = { ...row.fields }
  for (const f of hide) if (f in fields) fields[f] = { v: f === 'tagIds' ? [] : f === 'note' ? '' : null, h: fields[f]!.h }
  return { ...row, fields }
}

/** Ids of the members-only projects of a workspace that the person may not see. */
async function hiddenProjects(db: Db, user: Actor['user'], orgId: string, workspaceId: string): Promise<Set<string>> {
  const rows = await db
    .select({ rowId: schema.syncRow.rowId, fields: schema.syncRow.fields })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspaceId), eq(schema.syncRow.tableName, 'project')))
  const hidden = new Set<string>()
  for (const r of rows) {
    const fields = r.fields as Record<string, unknown>
    if (plain(fields.visibility) === 'members' && !canSeeProject(user, orgId, fields)) hidden.add(r.rowId)
  }
  return hidden
}

export async function visibleToCaller<T extends { workspaceId: string; table: string; fields: Record<string, { v: unknown; h: string }> }>(
  db: Db,
  actor: Actor['user'],
  workspaces: Map<string, string>,
  rows: T[],
): Promise<T[]> {
  const sharing = new Map<string, Set<string>>()
  const hidden = new Map<string, Set<string>>()
  const policies = new Map<string, PolicyRow[]>()
  const meetingCache = new Map<string, Map<string, Record<string, { v: unknown; h: string }>>>()
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
      const readAction = SHARED_READ[row.table]
      if (readAction && !actor.orgs[orgId]?.permissions[readAction]) continue
      if (row.table === 'project' && plain(row.fields.visibility) === 'members' && !canSeeProject(actor, orgId, row.fields)) continue
      // A tag of a project the person cannot see is not theirs to see either.
      const projectId = row.table === 'tag' ? plain(row.fields.projectId) : null
      if (typeof projectId === 'string') {
        let set = hidden.get(row.workspaceId)
        if (!set) {
          set = await hiddenProjects(db, actor, orgId, row.workspaceId)
          hidden.set(row.workspaceId, set)
        }
        if (set.has(projectId)) continue
      }
      out.push(row)
      continue
    }
    if (PUBLIC.has(row.table)) {
      out.push(row)
      continue
    }
    if (row.table === 'meeting' || row.table === 'meeting_response') {
      let meetings = meetingCache.get(row.workspaceId)
      if (!meetings) {
        meetings = await loadMeetings(db, row.workspaceId)
        meetingCache.set(row.workspaceId, meetings)
      }
      const fields = row.table === 'meeting' ? row.fields : meetings.get(String(plain(row.fields.meetingId)))
      if (fields && isAsked(fields, actor.id)) out.push(row)
      continue
    }
    const owner = typeof row.fields.userId?.v === 'string' ? (row.fields.userId.v as string) : null
    if (owner === actor.id) {
      out.push(row)
      continue
    }
    if (!owner) continue
    const scope = scopeOf({ user: actor, orgId }, READ[row.table] ?? 'member.read')
    if (scope === 'org') out.push(await limited(db, row, orgId, owner, policies))
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
      if (set.has(owner)) out.push(await limited(db, row, orgId, owner, policies))
    }
  }
  return out
}

