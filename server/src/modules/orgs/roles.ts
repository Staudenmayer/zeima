// The roles of an organization (SPEC §5.9): what each may do. The four
// built-in roles start from permissions.yaml; an admin can change the admin,
// manager and member roles and add roles of their own. The owner role is
// fixed. A person's role is a Keycloak group, so tokens carry it; what it
// allows is looked up here (and kept for a few seconds per organization).

import { eq } from 'drizzle-orm'
import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { type Db, schema } from '../../db/index.ts'
import { HttpProblem } from '../../plugins/errors.ts'
import type { OrgMembership } from '../../plugins/auth.ts'
import { type BuiltinRole, defaultPermissions, isBuiltinRole, ORG_ROLES, type Permissions, type RoleKey, type Scope, SCOPES } from '../../plugins/permissions.ts'

export interface RoleView {
  /** The Keycloak group name: a built-in role's name, or the id of the organization's own. */
  key: RoleKey
  name: string
  description: string
  builtin: boolean
  /** The owner role cannot be changed. */
  locked: boolean
  permissions: Permissions
}

const BUILTIN_NAMES: Record<BuiltinRole, string> = { owner: 'Owner', admin: 'Admin', manager: 'Manager', member: 'Member' }

const asPermissions = (raw: Record<string, string>): Permissions =>
  Object.fromEntries(Object.entries(raw).filter(([, s]) => (SCOPES as readonly string[]).includes(s))) as Record<string, Scope>

/** All roles of the organization: the built-in ones (with what an admin changed), then its own by name. */
export async function loadRoles(db: Db, orgId: string): Promise<RoleView[]> {
  const rows = await db.select().from(schema.orgRole).where(eq(schema.orgRole.orgId, orgId))
  const builtin: RoleView[] = ORG_ROLES.map((key) => {
    const row = rows.find((r) => r.key === key)
    return {
      key,
      name: BUILTIN_NAMES[key],
      description: row?.description ?? '',
      builtin: true,
      locked: key === 'owner',
      permissions: key !== 'owner' && row ? asPermissions(row.permissions) : defaultPermissions(key),
    }
  })
  const own: RoleView[] = rows
    .filter((r) => r.key === null)
    .map((r) => ({ key: r.id, name: r.name, description: r.description, builtin: false, locked: false, permissions: asPermissions(r.permissions) }))
    .sort((a, b) => a.name.localeCompare(b.name))
  return [...builtin, ...own]
}

export async function roleOf(db: Db, orgId: string, key: RoleKey): Promise<RoleView | null> {
  return (await loadRoles(db, orgId)).find((r) => r.key === key) ?? null
}

// ---------------------------------------------------------------- the cache
const TTL_MS = 10_000
const cache = new Map<string, { at: number; byKey: Map<RoleKey, Permissions> }>()

/** Call after a role changed, so this process sees it at once (others within seconds). */
export function forgetRoles(orgId: string) {
  cache.delete(orgId)
}

/** What a role of an organization allows now. */
export async function permissionsOfRole(db: Db, orgId: string, key: RoleKey): Promise<Permissions> {
  let entry = cache.get(orgId)
  if (!entry || Date.now() - entry.at > TTL_MS) {
    entry = { at: Date.now(), byKey: new Map((await loadRoles(db, orgId)).map((r) => [r.key, r.permissions])) }
    cache.set(orgId, entry)
  }
  return entry.byKey.get(key) ?? {}
}

// ------------------------------------------------------- who is a member now
// A token is good for minutes; the mirror of Keycloak's organizations is current. Membership, role and departments
// are taken from the mirror (kept for a few seconds per person), so someone who was removed or demoted loses access
// within that time and not when the token runs out.
interface Mirrored {
  role: RoleKey
  groups: string[]
}
const memberships = new Map<string, { at: number; orgs: Map<string, Mirrored> }>()
/** People remembered at most; a bigger crowd than this is served from the database. */
export const MEMBERSHIP_CACHE_MAX = 10_000

/** Call after a person joined, left, or changed role or department, so this process sees it at once (others within seconds). */
export function forgetMembership(userId: string) {
  memberships.delete(userId)
}

/** Call after the departments or the members of a whole organization changed (a department renamed or deleted, the organization deleted). */
export function forgetOrganizationMemberships(orgId: string) {
  for (const [userId, known] of memberships) if (known.orgs.has(orgId)) memberships.delete(userId)
}

/** Stores an entry and keeps the map bounded: expired entries go first, then the oldest. */
export function remember<V extends { at: number }>(map: Map<string, V>, key: string, value: V, max: number, ttlMs: number) {
  map.delete(key) // a refreshed entry counts as the newest
  if (map.size >= max) {
    const now = Date.now()
    for (const [k, v] of map) if (now - v.at > ttlMs) map.delete(k)
    // Insertion order is age order, so the first keys are the oldest.
    for (const k of map.keys()) {
      if (map.size < max) break
      map.delete(k)
    }
  }
  map.set(key, value)
}

async function mirroredOrgs(db: Db, userId: string, ttlMs: number): Promise<Map<string, Mirrored>> {
  const known = memberships.get(userId)
  if (known && Date.now() - known.at <= ttlMs) return known.orgs
  const orgs = new Map<string, Mirrored>()
  const rows = await db.select({ orgId: schema.kcOrgMember.orgId, role: schema.kcOrgMember.role }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, userId))
  for (const r of rows) orgs.set(r.orgId, { role: r.role, groups: [] })
  const groups = await db
    .select({ orgId: schema.kcGroup.orgId, path: schema.kcGroup.path })
    .from(schema.kcGroupMember)
    .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
    .where(eq(schema.kcGroupMember.userId, userId))
  for (const g of groups) orgs.get(g.orgId)?.groups.push(g.path)
  for (const o of orgs.values()) o.groups.sort()
  remember(memberships, userId, { at: Date.now(), orgs }, MEMBERSHIP_CACHE_MAX, ttlMs)
  return orgs
}

/** Gives each request's user what their roles allow in each organization they are a member of now. */
export const rolesPlugin = fp(
  async (app: FastifyInstance, options: { membershipTtlMs?: number }) => {
    const ttlMs = options.membershipTtlMs ?? TTL_MS
    app.addHook('onRequest', async (request) => {
      const user = request.user
      if (!user || !app.db) return
      // Only organizations the token names and the mirror still lists; role and departments as the mirror has them.
      const mirror = await mirroredOrgs(app.db, user.id, ttlMs)
      const orgs: Record<string, OrgMembership> = Object.create(null)
      for (const orgId of Object.keys(user.orgs)) {
        const now = mirror.get(orgId)
        if (!now) continue
        orgs[orgId] = { role: now.role, permissions: isBuiltinRole(now.role) ? defaultPermissions(now.role) : {}, groups: now.groups }
      }
      user.orgs = orgs
      if (request.orgId && !Object.hasOwn(orgs, request.orgId)) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
      for (const [orgId, membership] of Object.entries(user.orgs)) {
        // The owner is fixed; the others may have been changed by an admin.
        if (membership.role === 'owner') continue
        membership.permissions = await permissionsOfRole(app.db, orgId, membership.role)
      }
    })
  },
  { name: 'roles', dependencies: ['auth'] },
)
