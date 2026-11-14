// Organization policies as the server uses them (mirrors crates/zeima-core/src/policy.rs):
// rows of the `org_policy` table in the organization's workspace. A person's
// policy is the organization's, made stricter by that of each department
// they are in.

import { and, eq } from 'drizzle-orm'
import { type Db, schema } from '../../db/index.ts'

export type ManagerVisibility = 'full' | 'hide_notes' | 'totals_only'
export type PresenceVisibility = 'org' | 'off'

export interface Policy {
  requireProject: boolean
  requireTags: boolean
  managerVisibility: ManagerVisibility
  presenceVisibility: PresenceVisibility
  weeklyTargetMinutes: number | null
  autoPauseIdleMinutes: number | null
}

export interface PolicyRow {
  /** `null`: the whole organization; otherwise `dev/backend`. */
  groupPath: string | null
  settings: Partial<Policy>
}

export const DEFAULT_POLICY: Policy = {
  requireProject: false,
  requireTags: false,
  managerVisibility: 'full',
  presenceVisibility: 'org',
  weeklyTargetMinutes: null,
  autoPauseIdleMinutes: null,
}

const MANAGER_RANK: ManagerVisibility[] = ['full', 'hide_notes', 'totals_only']
const smaller = (a: number | null, b: number | null) => (a !== null && b !== null ? Math.min(a, b) : (a ?? b))

/** The stricter of two policies. */
export function stricter(a: Policy, b: Policy): Policy {
  return {
    requireProject: a.requireProject || b.requireProject,
    requireTags: a.requireTags || b.requireTags,
    managerVisibility: MANAGER_RANK.indexOf(b.managerVisibility) > MANAGER_RANK.indexOf(a.managerVisibility) ? b.managerVisibility : a.managerVisibility,
    presenceVisibility: a.presenceVisibility === 'off' || b.presenceVisibility === 'off' ? 'off' : 'org',
    weeklyTargetMinutes: smaller(a.weeklyTargetMinutes, b.weeklyTargetMinutes),
    autoPauseIdleMinutes: smaller(a.autoPauseIdleMinutes, b.autoPauseIdleMinutes),
  }
}

/** The policy of a person in the given departments (`dev/backend` style). */
export function effective(rows: readonly PolicyRow[], groups: readonly string[]): Policy {
  return rows
    .filter((r) => r.groupPath === null || groups.includes(r.groupPath))
    .reduce((acc, r) => stricter(acc, { ...DEFAULT_POLICY, ...r.settings }), DEFAULT_POLICY)
}

/** The policy rows of an organization's workspace. */
export async function loadPolicies(db: Db, workspaceId: string): Promise<PolicyRow[]> {
  const rows = await db
    .select({ fields: schema.syncRow.fields, deleted: schema.syncRow.deleted })
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspaceId), eq(schema.syncRow.tableName, 'org_policy')))
  const out: PolicyRow[] = []
  for (const r of rows) {
    const f = r.fields as Record<string, { v: unknown; h: string }>
    // A deleted policy is gone unless a field was written after the delete.
    if (r.deleted !== null && !Object.values(f).some((x) => x.h > r.deleted!)) continue
    const { groupPath, ...rest } = Object.fromEntries(Object.entries(f).map(([k, x]) => [k, x.v]))
    out.push({ groupPath: typeof groupPath === 'string' ? groupPath : null, settings: rest as Partial<Policy> })
  }
  return out
}

/** The departments of a person in an organization, as `dev/backend`. */
export async function departmentsOf(db: Db, orgId: string, userId: string): Promise<string[]> {
  const rows = await db
    .select({ path: schema.kcGroup.path })
    .from(schema.kcGroupMember)
    .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
    .where(and(eq(schema.kcGroup.orgId, orgId), eq(schema.kcGroupMember.userId, userId)))
  const prefix = `/orgs/${orgId}/groups/`
  return rows.map((r) => r.path).filter((p) => p.startsWith(prefix)).map((p) => p.slice(prefix.length))
}

/** The organization's own workspace id, if it has one. */
export async function orgWorkspaceId(db: Db, orgId: string): Promise<string | null> {
  const [row] = await db
    .select({ id: schema.workspace.id })
    .from(schema.workspace)
    .where(and(eq(schema.workspace.orgId, orgId), eq(schema.workspace.kind, 'org')))
  return row?.id ?? null
}

