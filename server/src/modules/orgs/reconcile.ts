// Brings the mirror tables (`kc_user`, `kc_org_member`, `kc_group`,
// `kc_group_member`) in line with Keycloak, which is the source of truth
// (SPEC §7.2). Runs at night, and when Keycloak says something changed
// (`POST /v1/internal/keycloak-events`), for one organization at a time.

import { and, eq, notInArray } from 'drizzle-orm'
import type { Db } from '../../db/index.ts'
import { schema } from '../../db/index.ts'
import type { KeycloakAdmin } from '../../keycloak/admin.ts'
import { ORG_ROLES, type RoleKey } from '../../plugins/permissions.ts'

export interface ReconcileResult {
  members: number
  addedMembers: number
  removedMembers: number
  groups: number
}

/** The role a person has, if any: the highest built-in one (owner > admin > manager > member), else one of the organization's own. */
function roleOf(userId: string, roles: Record<RoleKey, string[]>): RoleKey | null {
  const builtin = ORG_ROLES.find((r) => roles[r]?.includes(userId))
  if (builtin) return builtin
  return Object.entries(roles).find(([, ids]) => ids.includes(userId))?.[0] ?? null
}

export async function reconcileOrganization(db: Db, kc: KeycloakAdmin, orgId: string): Promise<ReconcileResult> {
  const [users, roles, departments] = await Promise.all([kc.listMembers(orgId), kc.listRoleMembers(orgId), kc.listDepartments(orgId)])
  const before = await db.select({ userId: schema.kcOrgMember.userId }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, orgId))
  const knownBefore = new Set(before.map((b) => b.userId))
  // Members Keycloak does not know of a role for are plain members.
  const rows = users.map((u) => ({ user: u, role: roleOf(u.id, roles) ?? 'member' }))

  await db.transaction(async (tx) => {
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
    const ids = users.map((u) => u.id)
    await tx
      .delete(schema.kcOrgMember)
      .where(and(eq(schema.kcOrgMember.orgId, orgId), ids.length ? notInArray(schema.kcOrgMember.userId, ids) : undefined))

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
  const now = new Set(users.map((u) => u.id))
  return {
    members: users.length,
    addedMembers: [...now].filter((id) => !knownBefore.has(id)).length,
    removedMembers: [...knownBefore].filter((id) => !now.has(id)).length,
    groups: departments.length,
  }
}

/** Reconciles every organization; one that fails does not stop the others. */
export async function reconcileAll(db: Db, kc: KeycloakAdmin, log: (msg: string, err?: unknown) => void): Promise<void> {
  const orgs = await db.select({ id: schema.organization.id }).from(schema.organization)
  for (const { id } of orgs) {
    try {
      const r = await reconcileOrganization(db, kc, id)
      if (r.addedMembers || r.removedMembers) log(`reconciled ${id}: +${r.addedMembers} -${r.removedMembers} members`)
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

