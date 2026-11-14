import { describe, expect, it } from 'vitest'
import type { AuthUser, OrgRole } from '../src/plugins/auth.ts'
import { type Actor, authorize, can, parseRules, rules, type Scope, scopeOf } from '../src/plugins/authz.ts'

const ORG = 'org-1'
const DEV = '/orgs/org-1/groups/dev'
const OPS = '/orgs/org-1/groups/ops'

function actor(role: OrgRole | null, groups: string[] = [], orgId: string | null = ORG): Actor {
  const user: AuthUser = {
    id: 'me',
    email: null,
    emailVerified: false,
    name: null,
    locale: null,
    roles: ['user'],
    orgs: role ? { [ORG]: { role, groups } } : {},
    client: 'zeima-desktop',
    expiresAt: Date.now() + 60_000,
  }
  return { user, orgId }
}

// The expected scope per action and role, written out independently of the
// YAML. Changing permissions.yaml means changing this table too.
const o = 'own', g = 'group', O = 'org', n = null
const MATRIX: Record<string, [owner: Scope | null, admin: Scope | null, manager: Scope | null, member: Scope | null]> = {
  'entry.read': [O, o, g, o],
  'entry.write': [o, o, o, o],
  'entry.write_locked': [O, O, g, n],
  'absence.read': [O, O, g, o],
  'absence.write': [o, o, o, o],
  'absence.approve': [O, O, g, n],
  'project.read': [O, O, O, O],
  'project.write': [O, O, n, n],
  'cost_center.read': [O, O, O, O],
  'cost_center.write': [O, O, n, n],
  'tag.write': [O, O, n, n],
  'policy.write': [O, O, n, n],
  'member.read': [O, O, g, o],
  'member.invite': [O, O, n, n],
  'member.manage': [O, O, n, n],
  'group.manage': [O, O, n, n],
  'role.assign': [O, n, n, n],
  'org.settings': [O, O, n, n],
  'addon.allowlist': [O, O, n, n],
  'audit.read': [O, O, n, n],
  'billing.manage': [O, n, n, n],
  'org.delete': [O, n, n, n],
}
const ROLES: OrgRole[] = ['owner', 'admin', 'manager', 'member']

describe('permissions.yaml', () => {
  it('has a matrix row for every action and no stale rows', () => {
    expect(Object.keys(rules).sort()).toEqual(Object.keys(MATRIX).sort())
  })

  for (const [action, scopes] of Object.entries(MATRIX)) {
    it(`${action}: ${scopes.map((s) => s ?? '-').join(' / ')} (owner / admin / manager / member)`, () => {
      for (const [i, role] of ROLES.entries()) expect(scopeOf(actor(role), action), role).toBe(scopes[i])
    })
  }

  it('rejects unknown roles and scopes', () => {
    expect(() => parseRules('version: 1\nactions:\n  a: { boss: org }')).toThrow(/unknown role/)
    expect(() => parseRules('version: 1\nactions:\n  a: { owner: world }')).toThrow(/unknown scope/)
    expect(() => parseRules('actions: {}')).toThrow(/version: 1/)
  })
})

describe('can()', () => {
  it('own: only the actor\'s own resources', () => {
    const member = actor('member')
    expect(can(member, 'entry.read', { orgId: ORG, ownerId: 'me' })).toBe(true)
    expect(can(member, 'entry.read', { orgId: ORG, ownerId: 'someone' })).toBe(false)
    expect(can(member, 'entry.read', { orgId: ORG })).toBe(false)
  })

  it('group: members of a shared department, and the manager\'s own', () => {
    const manager = actor('manager', [DEV])
    expect(can(manager, 'entry.read', { orgId: ORG, ownerId: 'a', ownerGroups: [DEV] })).toBe(true)
    expect(can(manager, 'entry.read', { orgId: ORG, ownerId: 'b', ownerGroups: [OPS] })).toBe(false)
    expect(can(manager, 'entry.read', { orgId: ORG, ownerId: 'b', ownerGroups: [] })).toBe(false)
    expect(can(manager, 'entry.read', { orgId: ORG, ownerId: 'me', ownerGroups: [] })).toBe(true)
  })

  it('org: anything in the active org, nothing in another org', () => {
    const owner = actor('owner')
    expect(can(owner, 'entry.read', { orgId: ORG, ownerId: 'x' })).toBe(true)
    expect(can(owner, 'entry.read', { orgId: 'org-2', ownerId: 'x' })).toBe(false)
  })

  it('denies without an active org, a membership or a known action', () => {
    expect(can(actor('owner', [], null), 'org.settings', { orgId: ORG })).toBe(false)
    expect(can(actor(null), 'org.settings', { orgId: ORG })).toBe(false)
    expect(can(actor('owner'), 'no.such.action', { orgId: ORG })).toBe(false)
  })

  it('authorize() throws a 403 problem', () => {
    expect(() => authorize(actor('member'), 'org.settings', { orgId: ORG })).toThrow(
      expect.objectContaining({ status: 403, code: 'forbidden' }),
    )
    expect(() => authorize(actor('admin'), 'org.settings', { orgId: ORG })).not.toThrow()
  })
})
