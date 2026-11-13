// Authorization policy (SPEC §5.9): `can(actor, action, resource)`.
//
// Authentication (auth.ts) says who the caller is and which role they have in
// the active org. This module decides what that role may do, from the rule
// table in packages/shared-fixtures/permissions.yaml, which the desktop app
// shares to hide or show UI. The server is authoritative.

import { readFileSync } from 'node:fs'
import { parse } from 'yaml'
import type { AuthUser, OrgRole } from './auth.ts'
import { ORG_ROLES } from './auth.ts'
import { HttpProblem } from './errors.ts'

export const SCOPES = ['own', 'group', 'org'] as const
export type Scope = (typeof SCOPES)[number]

export type Rules = Record<string, Partial<Record<OrgRole, Scope>>>

export const PERMISSIONS_PATH = new URL('../../../packages/shared-fixtures/permissions.yaml', import.meta.url)

/** Parses and checks a rule table; throws on unknown roles or scopes. */
export function parseRules(source: string): Rules {
  const doc = parse(source) as { version?: unknown; actions?: Record<string, Record<string, unknown>> } | null
  if (doc?.version !== 1 || !doc.actions || typeof doc.actions !== 'object') {
    throw new Error('permissions.yaml: expected `version: 1` and an `actions` map')
  }
  const rules: Rules = {}
  for (const [action, byRole] of Object.entries(doc.actions)) {
    const row: Partial<Record<OrgRole, Scope>> = {}
    for (const [role, scope] of Object.entries(byRole ?? {})) {
      if (!(ORG_ROLES as readonly string[]).includes(role)) throw new Error(`permissions.yaml: ${action}: unknown role '${role}'`)
      if (!(SCOPES as readonly string[]).includes(scope as string)) {
        throw new Error(`permissions.yaml: ${action}: unknown scope '${String(scope)}' for ${role}`)
      }
      row[role as OrgRole] = scope as Scope
    }
    rules[action] = row
  }
  return rules
}

export const rules: Rules = parseRules(readFileSync(PERMISSIONS_PATH, 'utf8'))

/** What the policy needs to know about the thing being accessed. */
export interface Resource {
  /** The org the resource belongs to; must be the actor's active org. */
  orgId: string
  /** Keycloak id of the user the resource belongs to (an entry's author). */
  ownerId?: string
  /** Department groups of that owner (full group paths). */
  ownerGroups?: readonly string[]
}

/** The caller as the policy sees them: a user in one org. */
export interface Actor {
  user: AuthUser
  orgId: string | null
}

/** Highest scope the actor's role grants for `action`, or null. */
export function scopeOf(actor: Actor, action: string): Scope | null {
  if (!actor.orgId) return null
  const role = actor.user.orgs[actor.orgId]?.role
  return (role && rules[action]?.[role]) || null
}

export function can(actor: Actor, action: string, resource: Resource): boolean {
  const scope = scopeOf(actor, action)
  const membership = actor.orgId ? actor.user.orgs[actor.orgId] : undefined
  if (!scope || !membership || resource.orgId !== actor.orgId) return false
  switch (scope) {
    case 'org':
      return true
    case 'own':
      return resource.ownerId !== undefined && resource.ownerId === actor.user.id
    case 'group':
      if (resource.ownerId !== undefined && resource.ownerId === actor.user.id) return true
      return resource.ownerGroups?.some((g) => membership.groups.includes(g)) ?? false
  }
}

/** Throws a 403 problem unless the actor may do `action` on `resource`. */
export function authorize(actor: Actor, action: string, resource: Resource): void {
  if (!can(actor, action, resource)) {
    throw new HttpProblem(403, 'forbidden', `not allowed: ${action}`)
  }
}
