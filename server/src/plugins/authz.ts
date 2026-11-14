// Authorization policy (SPEC §5.9): `can(actor, action, resource)`.
//
// Authentication (auth.ts) says who the caller is and which role they have in
// the active org. This module decides what that role may do, from the rule
// table in packages/shared-fixtures/permissions.yaml, which the desktop app
// shares to hide or show UI. The server is authoritative.

import type { AuthUser } from './auth.ts'
import { HttpProblem } from './errors.ts'
import { ALWAYS, parseRules, PERMISSIONS_PATH, rules, SCOPES, type Rules, type Scope } from './permissions.ts'

// The rule table and its parser live in permissions.ts; kept here for callers.
export { parseRules, PERMISSIONS_PATH, rules, SCOPES }
export type { Rules, Scope }

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
  const membership = actor.user.orgs[actor.orgId]
  if (!membership) return null
  return membership.permissions[action] ?? ALWAYS[action] ?? null
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
