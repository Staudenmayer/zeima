// Regression tests for the fourth penetration test (PT4-01, -02, -04).
import { describe, expect, it } from 'vitest'
import { occurrences, meetingShapeProblem } from '../src/meetings/occurrences.ts'
import { checkOrgWrite } from '../src/modules/sync/orgrules.ts'
import { orgsFromGroups } from '../src/plugins/auth.ts'

const orgId = '11111111-1111-4111-8111-111111111111'
const member = { id: 'u-member', orgs: orgsFromGroups([`/orgs/${orgId}/roles/member`]) } as never
const admin = { id: 'u-admin', orgs: orgsFromGroups([`/orgs/${orgId}/roles/admin`]) } as never
const stamp = (ms: number) => `${String(ms).padStart(15, '0')}-000000-dev`

describe('PT4-01 recurring meetings', () => {
  it('a long-running rule from far back is cheap to expand', () => {
    const start = Date.UTC(2000, 0, 1, 9)
    const from = Date.now()
    const t0 = performance.now()
    for (let i = 0; i < 20; i++) occurrences(start, start + 3600_000, 'Europe/Berlin', 'FREQ=DAILY;COUNT=999999', from, from + 31 * 86400_000)
    expect(performance.now() - t0).toBeLessThan(1500)
  })
  it('still finds the occurrences in the window', () => {
    const start = Date.UTC(2026, 0, 5, 9)
    const out = occurrences(start, start + 3600_000, 'UTC', 'FREQ=DAILY', Date.UTC(2026, 5, 1), Date.UTC(2026, 5, 4))
    expect(out.map(([s]) => new Date(s).toISOString())).toEqual(['2026-06-01T09:00:00.000Z', '2026-06-02T09:00:00.000Z', '2026-06-03T09:00:00.000Z'])
  })
  it('refuses meetings of an unreasonable shape', () => {
    const now = Date.now()
    const ok = (s = now, e = now + 3600_000, tz: unknown = 'UTC', r: unknown = null) => meetingShapeProblem(s, e, tz, r, now)
    expect(ok()).toBeNull()
    expect(ok(Date.UTC(2000, 0, 1), Date.UTC(2000, 0, 1) + 3600_000)).not.toBeNull()
    expect(ok(now, now - 1)).not.toBeNull()
    expect(ok(now, now + 3600_000, 'Not/AZone')).not.toBeNull()
    expect(ok(now, now + 3600_000, 'UTC', 'FREQ=DAILY;COUNT=999999')).not.toBeNull()
    expect(ok(now, now + 3600_000, 'UTC', 'garbage')).not.toBeNull()
    expect(ok(now, now + 3600_000, 'UTC', 'FREQ=WEEKLY;COUNT=52')).toBeNull()
  })
})

describe('PT4-02 shared rows stay shared', () => {
  const current = { fields: { profileId: { v: 'DE', h: stamp(1000) } }, deleted: null }
  const write = (user: never, fields: Record<string, unknown>) =>
    checkOrgWrite({} as never, { user, orgId }, orgId, { table: 'jurisdiction_selection', rowId: 'r', op: 'upsert', hlc: stamp(Date.now()), fields } as never, current as never)
  it('a member cannot turn the organization default into their own row', async () => {
    expect(await write(member, { userId: 'u-member', profileId: 'XX' })).toEqual({ status: 'rejected', reason: 'forbidden_field' })
    expect(await write(member, { profileId: 'XX' })).toEqual({ status: 'rejected', reason: 'forbidden' })
  })
  it('an administrator still edits the default', async () => {
    expect(await write(admin, { profileId: 'AT' })).toBeNull()
  })
})

describe('PT4-06 free/busy can be switched off by policy', () => {
  it('off in the organization or the department hides the person; the default shows', async () => {
    const { effective } = await import('../src/modules/sync/policy.ts')
    expect(effective([], []).planningVisibility).toBe('org')
    expect(effective([{ groupPath: null, settings: { planningVisibility: 'off' } }], []).planningVisibility).toBe('off')
    const dept = [{ groupPath: 'dev', settings: { planningVisibility: 'off' as const } }]
    expect(effective(dept, ['dev']).planningVisibility).toBe('off')
    expect(effective(dept, ['ops']).planningVisibility).toBe('org')
  })
})
