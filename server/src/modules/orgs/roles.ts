// The roles of an organization (SPEC §5.9): what each may do. The four
// built-in roles start from permissions.yaml; an admin can change the admin,
// manager and member roles and add roles of their own. The owner role is
// fixed. A person's role is a Keycloak group, so tokens carry it; what it
// allows is looked up here (and kept for a few seconds per organization).

import { eq } from 'drizzle-orm'
import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { type Db, schema } from '../../db/index.ts'
import { type BuiltinRole, defaultPermissions, ORG_ROLES, type Permissions, type RoleKey, type Scope, SCOPES } from '../../plugins/permissions.ts'

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

/** Gives each request's user what their roles allow in each organization. */
export const rolesPlugin = fp(
  async (app: FastifyInstance) => {
    app.addHook('onRequest', async (request) => {
      const user = request.user
      if (!user || !app.db) return
      for (const [orgId, membership] of Object.entries(user.orgs)) {
        // The owner is fixed; the others may have been changed by an admin.
        if (membership.role === 'owner') continue
        membership.permissions = await permissionsOfRole(app.db, orgId, membership.role)
      }
    })
  },
  { name: 'roles', dependencies: ['auth'] },
)
