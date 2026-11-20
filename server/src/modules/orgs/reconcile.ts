// Brings the mirror tables (`kc_user`, `kc_org_member`, `kc_group`,
// `kc_group_member`) in line with Keycloak, which is the source of truth
// (SPEC §7.2). Runs at night, and when Keycloak says something changed
// (`POST /v1/internal/keycloak-events`), for one organization at a time.

import { and, eq, isNull, notInArray } from 'drizzle-orm'
import type { Db } from '../../db/index.ts'
import { schema } from '../../db/index.ts'
import type { KeycloakAdmin } from '../../keycloak/admin.ts'
import { ORG_ROLES, type RoleKey } from '../../plugins/permissions.ts'
import { lockPushOrder } from '../../db/locks.ts'
import { forgetMembership } from './roles.ts'
import { purgeMemberOrgData, withOwnerLock } from './service.ts'

export interface ReconcileResult {
  members: number
  addedMembers: number
  removedMembers: number
  groups: number
  /** People who joined (through single sign-on) beyond the seats and were taken out again. */
  rejectedMembers: number
  /** People whose account was disabled and who were signed out. */
  signedOut: number
}

export interface ReconcileOptions {
  /** The seats the organization has; null is unlimited. Without it nobody is turned away. */
  seatLimit?: (orgId: string) => Promise<number | null>
}

/** The role a person has, if any: the highest built-in one (owner > admin > manager > member), else one of the organization's own. */
function roleOf(userId: string, roles: Record<RoleKey, string[]>): RoleKey | null {
  const builtin = ORG_ROLES.find((r) => roles[r]?.includes(userId))
  if (builtin) return builtin
  return Object.entries(roles).find(([, ids]) => ids.includes(userId))?.[0] ?? null
}

export async function reconcileOrganization(db: Db, kc: KeycloakAdmin, orgId: string, opts: ReconcileOptions = {}): Promise<ReconcileResult> {
  // Keycloak announces a new organization before the API has stored it (it is still being created):
  // there is nothing to compare yet, and the creation fills the copy itself.
  const [known] = await db.select({ id: schema.organization.id }).from(schema.organization).where(eq(schema.organization.id, orgId))
  if (!known) return { members: 0, addedMembers: 0, removedMembers: 0, groups: 0, rejectedMembers: 0, signedOut: 0 }
  // Keycloak is read and the mirror written under the organization lock that the routes changing members and roles
  // take, so a change made in between is not undone by a list read before it. The lock is held for those two steps only.
  const done = await withOwnerLock(db, orgId, async (tx) => {
    const [listed, roles, departments] = await Promise.all([kc.listMembers(orgId), kc.listRoleMembers(orgId), kc.listDepartments(orgId)])
    const before = await tx
      .select({ userId: schema.kcOrgMember.userId, active: schema.kcUser.active })
      .from(schema.kcOrgMember)
      .innerJoin(schema.kcUser, eq(schema.kcUser.id, schema.kcOrgMember.userId))
      .where(eq(schema.kcOrgMember.orgId, orgId))
    const memberCount = await tx.$count(schema.kcOrgMember, eq(schema.kcOrgMember.orgId, orgId))
    const knownBefore = new Set(before.map((b) => b.userId))
    const wasActive = new Set(before.filter((b) => b.active).map((b) => b.userId))

    // People who joined without an invitation (single sign-on, SCIM) may only take seats that are free.
    const rejected = await turnAwayBeyondSeats(tx, kc, orgId, listed.filter((u) => !knownBefore.has(u.id)), memberCount, opts)
    const users = listed.filter((u) => !rejected.has(u.id))
    // Members Keycloak does not know of a role for are plain members.
    const rows = users.map((u) => ({ user: u, role: roleOf(u.id, roles) ?? 'member' }))

    const ids = users.map((u) => u.id)
    const gone = [...knownBefore].filter((id) => !ids.includes(id))
    await tx.transaction(async (tx) => {
      // Pushes finish first or see the memberships gone (only taken when somebody leaves).
      if (gone.length > 0) await lockPushOrder(tx)
      for (const { user, role } of rows) {
        await tx
          .insert(schema.kcUser)
          .values({ id: user.id, email: user.email, displayName: user.name, active: user.enabled })
          .onConflictDoUpdate({
            target: schema.kcUser.id,
            // A name from a token is better than none; do not blank it.
            set: { email: user.email, active: user.enabled, ...(user.name ? { displayName: user.name } : {}) },
          })
        await tx
          .insert(schema.kcOrgMember)
          .values({ orgId, userId: user.id, role })
          .onConflictDoUpdate({ target: [schema.kcOrgMember.orgId, schema.kcOrgMember.userId], set: { role } })
      }
      await tx
        .delete(schema.kcOrgMember)
        .where(and(eq(schema.kcOrgMember.orgId, orgId), ids.length ? notInArray(schema.kcOrgMember.userId, ids) : undefined))
      for (const userId of gone) await purgeMemberOrgData(tx, orgId, userId)

      // Departments: the same ids and paths as Keycloak, with their members.
      const groupIds = departments.map((d) => d.id)
      await tx.delete(schema.kcGroup).where(and(eq(schema.kcGroup.orgId, orgId), groupIds.length ? notInArray(schema.kcGroup.id, groupIds) : undefined))
      for (const d of departments) {
        await tx
          .insert(schema.kcGroup)
          .values({ id: d.id, orgId, path: d.path })
          .onConflictDoUpdate({ target: schema.kcGroup.id, set: { path: d.path } })
        await tx.delete(schema.kcGroupMember).where(eq(schema.kcGroupMember.groupId, d.id))
        const members = d.memberIds.filter((m) => users.some((u) => u.id === m))
        if (members.length) await tx.insert(schema.kcGroupMember).values(members.map((userId) => ({ groupId: d.id, userId })))
      }
    })
    return { users, rejected, ids, gone, knownBefore, wasActive, departments }
  })
  const { users, rejected, ids, gone, knownBefore, wasActive, departments } = done
  for (const id of new Set([...ids, ...gone])) forgetMembership(id)
  // A disabled account stops working now, not when its token runs out.
  let signedOut = 0
  for (const u of users) {
    if (!u.enabled && wasActive.has(u.id)) {
      await kc.logoutUser(u.id).then(
        () => void signedOut++,
        () => {}, // the person has no session; or Keycloak is busy and the next check tries again
      )
    }
  }
  const now = new Set(users.map((u) => u.id))
  return {
    rejectedMembers: rejected.size,
    signedOut,
    members: users.length,
    addedMembers: [...now].filter((id) => !knownBefore.has(id)).length,
    removedMembers: [...knownBefore].filter((id) => !now.has(id)).length,
    groups: departments.length,
  }
}

/**
 * Takes the newcomers beyond the free seats out of the organization (in Keycloak; the mirror never saw them).
 * A pending invitation holds a seat for its address, so the invitee is let in; anyone else needs a free one.
 */
async function turnAwayBeyondSeats(
  db: Db,
  kc: KeycloakAdmin,
  orgId: string,
  newcomers: { id: string; email: string | null }[],
  members: number,
  opts: ReconcileOptions,
): Promise<Set<string>> {
  const out = new Set<string>()
  const limit = newcomers.length && opts.seatLimit ? await opts.seatLimit(orgId) : null
  if (limit === null) return out
  const pending = await db
    .select({ email: schema.orgInvitation.email })
    .from(schema.orgInvitation)
    .where(and(eq(schema.orgInvitation.orgId, orgId), isNull(schema.orgInvitation.acceptedAt)))
  const invited = new Set(pending.map((p) => p.email.toLowerCase()))
  let free = limit - members - pending.length
  for (const u of newcomers) {
    if (u.email && invited.has(u.email.toLowerCase())) continue
    if (free > 0) {
      free--
      continue
    }
    try {
      await kc.removeMember(orgId, u.id)
      out.add(u.id)
      await db.insert(schema.auditLog).values({ orgId, actorId: null, action: 'member.rejected', target: u.id, detail: { reason: 'seat_limit', limit }, at: Date.now() })
    } catch {
      // Keycloak is busy: the person stays for now and the next check tries again.
    }
  }
  return out
}

/** Reconciles every organization; one that fails does not stop the others. */
export async function reconcileAll(db: Db, kc: KeycloakAdmin, log: (msg: string, err?: unknown) => void, opts: ReconcileOptions = {}): Promise<void> {
  const orgs = await db.select({ id: schema.organization.id }).from(schema.organization)
  for (const { id } of orgs) {
    try {
      const r = await reconcileOrganization(db, kc, id, opts)
      if (r.addedMembers || r.removedMembers || r.rejectedMembers) log(`reconciled ${id}: +${r.addedMembers} -${r.removedMembers} members, ${r.rejectedMembers} turned away`)
    } catch (err) {
      log(`could not reconcile ${id}`, err)
    }
  }
}

/** Organizations the user belongs to (mirror), for an event about one person. */
export async function orgsOfUser(db: Db, userId: string): Promise<string[]> {
  const rows = await db.select({ orgId: schema.kcOrgMember.orgId }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, userId))
  return rows.map((r) => r.orgId)
}

