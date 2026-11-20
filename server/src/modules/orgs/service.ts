// Organization management (SPEC §5.9): what the routes do. Changes go to
// Keycloak first and then to the mirror tables, so a failed call leaves the
// mirror as it was.

import { randomBytes, randomUUID } from 'node:crypto'
import { and, asc, desc, eq, gt, inArray, isNull, lt, sql } from 'drizzle-orm'
import type { Db } from '../../db/index.ts'
import { schema } from '../../db/index.ts'
import type { KeycloakAdmin } from '../../keycloak/admin.ts'
import { HttpProblem } from '../../plugins/errors.ts'
import { covers } from '../../plugins/permissions.ts'
import { forgetMembership, permissionsOfRole, roleOf } from './roles.ts'

export interface MemberRow {
  id: string
  email: string | null
  name: string | null
  role: string
  active: boolean
  lastSeenAt: number | null
}

export const slug = (name: string) =>
  name
    .normalize('NFKD')
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/[^a-zA-Z0-9-]/g, '')
    .replace(/^-+|-+$/g, '')
    .toLowerCase()
    .slice(0, 40) || 'org'

export async function audit(db: Db, entry: { orgId: string; actorId: string | null; action: string; target?: string | null; detail?: object }) {
  await db.insert(schema.auditLog).values({
    orgId: entry.orgId,
    actorId: entry.actorId,
    action: entry.action,
    target: entry.target ?? null,
    detail: entry.detail ?? {},
    at: Date.now(),
  })
}

/** The workspace the organization's shared data lives in (one per organization). */
export async function ensureOrgWorkspace(db: Db, orgId: string, name: string): Promise<string> {
  const now = Date.now()
  await db
    .insert(schema.workspace)
    .values({
      id: randomUUID(),
      kind: 'org',
      orgId,
      name,
      createdAt: now,
      updatedAt: now,
      hlc: `${String(now).padStart(15, '0')}-000000-server`,
      originDevice: orgId,
    })
    .onConflictDoNothing()
  const [row] = await db.select({ id: schema.workspace.id }).from(schema.workspace).where(and(eq(schema.workspace.orgId, orgId), eq(schema.workspace.kind, 'org')))
  return row!.id
}

/** Deletes the organization and everything in its shared workspace. */
export async function deleteOrganization(db: Db, orgId: string) {
  await db.transaction(async (tx) => {
    const ws = sql`(select id from workspace where org_id = ${orgId})`
    await tx.execute(sql`delete from time_entry_tag where entry_id in (select id from time_entry where workspace_id in ${ws})`)
    for (const table of ['pause', 'time_entry', 'tag', 'project', 'sync_row']) {
      await tx.execute(sql`delete from ${sql.identifier(table)} where workspace_id in ${ws}`)
    }
    await tx.execute(sql`delete from workspace where org_id = ${orgId}`)
    await tx.delete(schema.organization).where(eq(schema.organization.id, orgId))
  })
}

/**
 * Deletes one person's data in an organization's shared workspace: their entries, pauses and the sync state of
 * both. Shared projects and tags, other members' rows and the person's personal workspace stay.
 * Call it in the transaction that removes the membership.
 */
export async function purgeMemberOrgData(db: Pick<Db, 'execute'>, orgId: string, userId: string) {
  const ws = sql`(select id from workspace where org_id = ${orgId})`
  await db.execute(sql`delete from sync_row where workspace_id in ${ws} and table_name in ('time_entry', 'pause') and (fields -> 'userId' ->> 'v' = ${userId} or row_id in (select id from time_entry where workspace_id in ${ws} and user_id = ${userId} union select id from pause where workspace_id in ${ws} and user_id = ${userId}))`)
  await db.execute(sql`delete from time_entry_tag where entry_id in (select id from time_entry where workspace_id in ${ws} and user_id = ${userId})`)
  await db.execute(sql`delete from pause where workspace_id in ${ws} and user_id = ${userId}`)
  await db.execute(sql`delete from time_entry where workspace_id in ${ws} and user_id = ${userId}`)
}

export async function createOrganization(
  db: Db,
  kc: KeycloakAdmin,
  creator: { id: string; email: string | null },
  name: string,
): Promise<{ id: string; name: string }> {
  const alias = `${slug(name)}-${randomBytes(3).toString('hex')}`
  const id = await kc.createOrganization({ name, alias })
  try {
    await kc.addMember(id, creator.id)
    await kc.setRole(id, creator.id, 'owner')
    await db.transaction(async (tx) => {
      await tx.insert(schema.organization).values({ id, name, createdAt: Date.now() })
      await tx.insert(schema.kcOrgMember).values({ orgId: id, userId: creator.id, role: 'owner' })
      await ensureOrgWorkspace(tx, id, name)
    })
  } catch (err) {
    await kc.deleteOrganization(id).catch(() => {})
    throw err
  }
  forgetMembership(creator.id)
  await audit(db, { orgId: id, actorId: creator.id, action: 'org.create', target: id, detail: { name } })
  return { id, name }
}

export async function listMembers(db: Db, orgId: string, only?: { userIds: readonly string[] }): Promise<MemberRow[]> {
  const rows = await db
    .select({
      id: schema.kcUser.id,
      email: schema.kcUser.email,
      name: schema.kcUser.displayName,
      role: schema.kcOrgMember.role,
      active: schema.kcUser.active,
      lastSeenAt: schema.kcUser.lastSeenAt,
    })
    .from(schema.kcOrgMember)
    .innerJoin(schema.kcUser, eq(schema.kcUser.id, schema.kcOrgMember.userId))
    .where(and(eq(schema.kcOrgMember.orgId, orgId), only ? inArray(schema.kcOrgMember.userId, [...only.userIds]) : undefined))
    .orderBy(asc(sql`lower(coalesce(${schema.kcUser.displayName}, ${schema.kcUser.email}, ''))`))
  return rows as MemberRow[]
}

/** The profile pictures (data URLs) of these users; those without one are missing from the map. */
export async function avatarsOf(db: Db, userIds: readonly string[]): Promise<Map<string, string>> {
  if (userIds.length === 0) return new Map()
  const rows = await db.select({ id: schema.kcUser.id, avatar: schema.kcUser.avatar }).from(schema.kcUser).where(inArray(schema.kcUser.id, [...userIds]))
  return new Map(rows.flatMap((r) => (r.avatar ? [[r.id, r.avatar] as const] : [])))
}

export async function memberOf(db: Db, orgId: string, userId: string): Promise<MemberRow | null> {
  return (await listMembers(db, orgId, { userIds: [userId] }))[0] ?? null
}

/** Users who share a department group with any of `groupPaths`. */
export async function sharingGroups(db: Db, orgId: string, groupPaths: readonly string[]): Promise<string[]> {
  if (groupPaths.length === 0) return []
  const rows = await db
    .selectDistinct({ userId: schema.kcGroupMember.userId })
    .from(schema.kcGroupMember)
    .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
    .where(and(eq(schema.kcGroup.orgId, orgId), inArray(schema.kcGroup.path, [...groupPaths])))
  return rows.map((r) => r.userId)
}

/** Seats in use: members plus pending invitations. `limit` is null when unlimited. */
export async function seatUsage(db: Db, orgId: string) {
  const [members] = await db.select({ n: sql<number>`count(*)::int` }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, orgId))
  const [pending] = await db
    .select({ n: sql<number>`count(*)::int` })
    .from(schema.orgInvitation)
    .where(and(eq(schema.orgInvitation.orgId, orgId), isNull(schema.orgInvitation.acceptedAt), gt(schema.orgInvitation.createdAt, Date.now() - INVITATION_TTL_MS)))
  return { members: members!.n, pending: pending!.n }
}

export function requireSeat(limit: number | null, usage: { members: number; pending: number }) {
  if (limit !== null && usage.members + usage.pending >= limit) {
    throw new HttpProblem(409, 'seat_limit_reached', `all ${limit} seats are in use; remove a member or invitation, or get more seats`)
  }
}

/**
 * Runs `work` while holding the organization's owner lock. "An organization keeps at least one owner" is a check
 * of the count followed by a change; two owners demoting each other at the same instant would both see two owners
 * and leave none. Changes to who owns an organization take this lock (released at the end of the transaction), so
 * the second one sees the first one's result.
 */
export async function withOwnerLock<T>(db: Db, orgId: string, work: (tx: Db) => Promise<T>): Promise<T> {
  return db.transaction(async (tx) => {
    await tx.execute(sql`select pg_advisory_xact_lock(hashtext(${`owners:${orgId}`}))`)
    return work(tx as unknown as Db)
  })
}

export async function ownerCount(db: Db, orgId: string): Promise<number> {
  const [row] = await db
    .select({ n: sql<number>`count(*)::int` })
    .from(schema.kcOrgMember)
    .where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.role, 'owner')))
  return row!.n
}

export async function pendingInvitations(db: Db, orgId: string) {
  return db
    .select()
    .from(schema.orgInvitation)
    .where(and(eq(schema.orgInvitation.orgId, orgId), isNull(schema.orgInvitation.acceptedAt)))
    .orderBy(desc(schema.orgInvitation.createdAt))
}

export async function auditEntries(db: Db, orgId: string, opts: { before?: number; limit: number }) {
  return db
    .select()
    .from(schema.auditLog)
    .where(and(eq(schema.auditLog.orgId, orgId), opts.before ? lt(schema.auditLog.id, opts.before) : undefined))
    .orderBy(desc(schema.auditLog.id))
    .limit(opts.limit)
}

/**
 * Gives a user the role of an invitation they have accepted: Keycloak has
 * already made them a member of the organization (they used the invitation
 * link); this adds the role group and the mirror row. Needs a verified
 * email, so nobody can claim an invitation by typing someone else's address.
 */
/** An invitation not answered within this time is void. */
export const INVITATION_TTL_MS = 14 * 86_400_000

export async function acceptInvitations(db: Db, kc: KeycloakAdmin, user: { id: string; email: string | null; emailVerified: boolean }) {
  if (!user.email || !user.emailVerified) return
  const email = user.email.toLowerCase()
  const pending = await db
    .select()
    .from(schema.orgInvitation)
    .where(and(eq(schema.orgInvitation.email, email), isNull(schema.orgInvitation.acceptedAt), gt(schema.orgInvitation.createdAt, Date.now() - INVITATION_TTL_MS)))
  for (const invitation of pending) {
    // The inviter must still be there, and still be allowed to give this role; otherwise the person joins as a member.
    const [inviter] = invitation.invitedBy
      ? await db.select({ role: schema.kcOrgMember.role }).from(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, invitation.orgId), eq(schema.kcOrgMember.userId, invitation.invitedBy)))
      : []
    const known = invitation.role === 'member' || (await db.select({ id: schema.orgRole.id }).from(schema.orgRole).where(and(eq(schema.orgRole.orgId, invitation.orgId), invitation.role.length > 30 ? eq(schema.orgRole.id, invitation.role) : eq(schema.orgRole.key, invitation.role)))).length > 0 || ['admin', 'manager', 'owner'].includes(invitation.role)
    // Looked at again now: what the inviter holds today, not when they invited.
    let mayGive = false
    if (inviter) {
      if (invitation.role === 'member') mayGive = true
      else if (['admin', 'owner'].includes(invitation.role)) mayGive = inviter.role === 'owner'
      else if (inviter.role === 'owner') mayGive = true
      else {
        const inviterPermissions = await permissionsOfRole(db, invitation.orgId, inviter.role)
        const target = await roleOf(db, invitation.orgId, invitation.role)
        mayGive = Boolean(target) && covers(inviterPermissions, target!.permissions) && !('role.edit' in target!.permissions || 'role.assign' in target!.permissions)
      }
    }
    const role = mayGive && known ? invitation.role : 'member'
    await kc.addMember(invitation.orgId, user.id)
    await kc.setRole(invitation.orgId, user.id, role)
    // The departments the inviter chose (those that still exist in the organization).
    // Departments are placed by those who manage departments.
    const inviterMayPlace = inviter && (inviter.role === 'owner' || (await permissionsOfRole(db, invitation.orgId, inviter.role))['group.manage'] === 'org')
    const groups = invitation.groupIds.length && inviterMayPlace
      ? await db
          .select({ id: schema.kcGroup.id })
          .from(schema.kcGroup)
          .where(and(eq(schema.kcGroup.orgId, invitation.orgId), inArray(schema.kcGroup.id, invitation.groupIds)))
      : []
    for (const g of groups) await kc.addToGroup(user.id, g.id)
    await db.transaction(async (tx) => {
      await tx
        .insert(schema.kcOrgMember)
        .values({ orgId: invitation.orgId, userId: user.id, role, jobId: invitation.jobId })
        .onConflictDoNothing()
      for (const g of groups) await tx.insert(schema.kcGroupMember).values({ groupId: g.id, userId: user.id }).onConflictDoNothing()
      await tx.update(schema.orgInvitation).set({ acceptedAt: Date.now() }).where(eq(schema.orgInvitation.id, invitation.id))
    })
    forgetMembership(user.id)
    await audit(db, { orgId: invitation.orgId, actorId: user.id, action: 'member.join', target: user.id, detail: { role } })
  }
}
