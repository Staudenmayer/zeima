// The permissions an organization role is made of (SPEC §5.9).
//
// A role is a map of action → scope (`own`, `group`, `org`); an action missing
// from it is denied. The four built-in roles come from
// packages/shared-fixtures/permissions.yaml; an organization's admins change
// them and add roles of their own (the API's `org_role` table). The catalog
// says which actions a role can be given, grouped by what they act on and by
// whether they view, edit or delete it, for the role editor.

import { readFileSync } from 'node:fs'
import { parse } from 'yaml'

export const ORG_ROLES = ['owner', 'admin', 'manager', 'member'] as const
/** The four roles every organization starts with. */
export type BuiltinRole = (typeof ORG_ROLES)[number]
/** A built-in role's name, or the id of a role the organization made. */
export type RoleKey = string
export const isBuiltinRole = (key: string): key is BuiltinRole => (ORG_ROLES as readonly string[]).includes(key)

export const SCOPES = ['own', 'group', 'org'] as const
export type Scope = (typeof SCOPES)[number]
const RANK: Record<Scope, number> = { own: 0, group: 1, org: 2 }

/** What a role grants: action → the widest scope. */
export type Permissions = Record<string, Scope>

export type Kind = 'view' | 'edit' | 'delete'

export interface PermissionDef {
  action: string
  /** What it acts on (a key for the role editor's labels). */
  resource: string
  kind: Kind
  /** The scopes a role can be given: one (`org`) is a plain yes/no. */
  scopes: Scope[]
}

const ORG: Scope[] = ['org']
const ALL: Scope[] = ['own', 'group', 'org']
const GROUP_ORG: Scope[] = ['group', 'org']
const def = (resource: string, kind: Kind, action: string, scopes: Scope[] = ORG): PermissionDef => ({ action, resource, kind, scopes })

export const CATALOG: PermissionDef[] = [
  def('entries', 'view', 'entry.read', ALL),
  def('entries', 'edit', 'entry.write_locked', GROUP_ORG),
  def('absences', 'view', 'absence.read', ALL),
  def('absences', 'edit', 'absence.approve', GROUP_ORG),
  def('projects', 'view', 'project.read'),
  def('projects', 'edit', 'project.write'),
  def('projects', 'delete', 'project.delete'),
  def('costCenters', 'view', 'cost_center.read'),
  def('costCenters', 'edit', 'cost_center.write'),
  def('costCenters', 'delete', 'cost_center.delete'),
  def('tags', 'edit', 'tag.write'),
  def('tags', 'delete', 'tag.delete'),
  def('rules', 'edit', 'policy.write'),
  def('rules', 'delete', 'policy.delete'),
  def('members', 'view', 'member.read', ALL),
  def('members', 'edit', 'member.invite'),
  def('members', 'edit', 'member.manage'),
  def('members', 'delete', 'member.remove'),
  def('departments', 'edit', 'group.manage'),
  def('departments', 'delete', 'group.delete'),
  def('jobs', 'edit', 'job.edit'),
  def('jobs', 'delete', 'job.delete'),
  def('roles', 'view', 'role.view'),
  def('roles', 'edit', 'role.edit'),
  def('roles', 'edit', 'role.assign'),
  def('roles', 'delete', 'role.delete'),
  def('organization', 'view', 'audit.read'),
  def('organization', 'edit', 'org.settings'),
  def('organization', 'edit', 'addon.allowlist'),
  def('organization', 'edit', 'billing.manage'),
  def('organization', 'delete', 'org.delete'),
]

/** Everybody in an organization may do these with their own things; no role takes them away. */
export const ALWAYS: Record<string, Scope> = { 'entry.write': 'own', 'absence.write': 'own' }

export type Rules = Record<string, Partial<Record<BuiltinRole, Scope>>>

export const PERMISSIONS_PATH = new URL('../../../packages/shared-fixtures/permissions.yaml', import.meta.url)

/** Parses and checks a rule table; throws on unknown roles or scopes. */
export function parseRules(source: string): Rules {
  const doc = parse(source) as { version?: unknown; actions?: Record<string, Record<string, unknown>> } | null
  if (doc?.version !== 1 || !doc.actions || typeof doc.actions !== 'object') {
    throw new Error('permissions.yaml: expected `version: 1` and an `actions` map')
  }
  const rules: Rules = {}
  for (const [action, byRole] of Object.entries(doc.actions)) {
    const row: Partial<Record<BuiltinRole, Scope>> = {}
    for (const [role, scope] of Object.entries(byRole ?? {})) {
      if (!isBuiltinRole(role)) throw new Error(`permissions.yaml: ${action}: unknown role '${role}'`)
      if (!(SCOPES as readonly string[]).includes(scope as string)) {
        throw new Error(`permissions.yaml: ${action}: unknown scope '${String(scope)}' for ${role}`)
      }
      row[role] = scope as Scope
    }
    rules[action] = row
  }
  return rules
}

export const rules: Rules = parseRules(readFileSync(PERMISSIONS_PATH, 'utf8'))

/** The permissions of a built-in role as the table has them. */
export function defaultPermissions(role: BuiltinRole): Permissions {
  const out: Permissions = {}
  for (const [action, byRole] of Object.entries(rules)) {
    const scope = byRole[role]
    if (scope) out[action] = scope
  }
  return out
}

/** Whether `have` covers everything in `want` (a role may only hand out what its giver holds). */
export function covers(have: Permissions, want: Permissions): boolean {
  return Object.entries(want).every(([action, scope]) => {
    const own = have[action]
    return own !== undefined && RANK[own] >= RANK[scope]
  })
}

/** Checks a role's permissions against the catalog; returns the cleaned map or throws. */
export function cleanPermissions(input: Record<string, unknown>): Permissions {
  const out: Permissions = {}
  for (const [action, scope] of Object.entries(input)) {
    const known = CATALOG.find((d) => d.action === action)
    if (!known) throw new Error(`unknown permission '${action}'`)
    if (typeof scope !== 'string' || !(SCOPES as readonly string[]).includes(scope)) throw new Error(`unknown scope for '${action}'`)
    if (!known.scopes.includes(scope as Scope)) throw new Error(`'${action}' cannot be '${scope}'`)
    out[action] = scope as Scope
  }
  return out
}
