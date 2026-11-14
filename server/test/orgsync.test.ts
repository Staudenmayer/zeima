// Shared workspace of an organization: who may write and see which rows.

import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { schema } from '../src/db/index.ts'
import { testApp, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const OWNER = id(1), ADMIN = id(2), MANAGER = id(3), ANNA = id(4), BEN = id(5), OUTSIDER = id(6)
const ORG = id(900), DEVICE = id(901)
const DEV = `/orgs/${ORG}/groups/dev`
const hlc = (wall: number) => `${String(wall).padStart(15, '0')}-000000-a`
const ROLES: Record<string, string> = { [OWNER]: 'owner', [ADMIN]: 'admin', [MANAGER]: 'manager', [ANNA]: 'member', [BEN]: 'member' }
/** Departments in the token: the manager and Anna are in `dev`, Ben is not. */
const GROUPS: Record<string, string[]> = { [MANAGER]: [DEV], [ANNA]: [DEV] }

describe.skipIf(!(await reachable()))('organization workspace', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  let app: Awaited<ReturnType<typeof testApp>>
  let ws: string

  const headers = async (sub: string) => ({
    authorization: `Bearer ${await token({ sub, email: `${sub.slice(-1)}@example.com`, groups: ROLES[sub] ? [`/orgs/${ORG}/roles/${ROLES[sub]}`, ...(GROUPS[sub] ?? [])] : [] })}`,
  })
  const push = async (sub: string, changes: unknown[]) =>
    app.inject({ method: 'POST', url: '/v1/sync/push', headers: await headers(sub), payload: { workspaceId: ws, deviceId: DEVICE, changes } })
  const upsert = (table: string, rowId: string, h: number, fields: Record<string, unknown>) => ({ table, rowId, op: 'upsert', hlc: hlc(h), fields })
  const results = async (sub: string, changes: unknown[]) => (await push(sub, changes)).json().results as { status: string; reason?: string }[]
  const pulledIds = async (sub: string) =>
    ((await app.inject({ url: '/v1/sync/pull', headers: await headers(sub) })).json().rows as { rowId: string; workspaceId: string }[])
      .filter((r) => r.workspaceId === ws)
      .map((r) => r.rowId)
      .sort()

  beforeAll(async () => {
    db = await createTestDatabase()
    app = await testApp({ db: db.db })
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    for (const [uid, role] of Object.entries(ROLES)) {
      await db.db.insert(schema.kcUser).values({ id: uid, email: `${uid.slice(-1)}@example.com`, displayName: uid.slice(-1) })
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: uid, role })
    }
    await db.db.insert(schema.kcGroup).values({ id: id(800), orgId: ORG, path: DEV })
    await db.db.insert(schema.kcGroupMember).values([{ groupId: id(800), userId: MANAGER }, { groupId: id(800), userId: ANNA }])
  })
  afterAll(async () => {
    await app?.close()
    await db?.drop()
  })

  it('gives every organization one shared workspace, visible to its members only', async () => {
    const list = async (sub: string) => (await app.inject({ url: '/v1/workspaces', headers: await headers(sub) })).json() as { id: string; kind: string; orgId: string | null }[]
    const first = await list(ANNA)
    expect(first).toHaveLength(1)
    expect(first[0]).toMatchObject({ kind: 'org', orgId: ORG, name: 'Acme' })
    ws = first[0]!.id
    expect((await list(BEN))[0]!.id).toBe(ws)
    expect(await list(OUTSIDER)).toEqual([])
    expect(await db.db.select().from(schema.workspace).where(eq(schema.workspace.orgId, ORG))).toHaveLength(1)
  })

  it('lets only people with the right change shared data', async () => {
    const P = id(10), T = id(11), CC = id(12)
    expect(await results(ADMIN, [upsert('project', P, 10, { name: 'Website' }), upsert('tag', T, 10, { name: 'urgent' })])).toEqual([{ status: 'applied' }, { status: 'applied' }])
    expect(await results(ANNA, [upsert('project', P, 20, { name: 'Hacked' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    expect(await results(MANAGER, [upsert('tag', T, 20, { name: 'x' }), upsert('cost_center', CC, 20, { code: '1' })])).toEqual([
      { status: 'rejected', reason: 'forbidden' },
      { status: 'rejected', reason: 'forbidden' },
    ])
    expect(await results(OWNER, [upsert('cost_center', CC, 20, { code: '100' })])).toEqual([{ status: 'applied' }])
    // Everyone in the organization reads them; outsiders cannot push at all.
    for (const who of [ANNA, BEN, MANAGER]) expect(await pulledIds(who)).toEqual([P, T, CC].sort())
    expect((await push(OUTSIDER, [])).statusCode).toBe(404)
  })

  it('keeps display settings out of the shared workspace', async () => {
    expect(await results(OWNER, [upsert('display_settings', id(13), 10, { theme: 'dark' })])).toEqual([{ status: 'rejected', reason: 'forbidden_table' }])
  })

  it('hides members-only projects from people who are not on them', async () => {
    const SECRET = id(30), OPEN = id(31)
    // The admin creates one open project and one for Anna's team, managed by Anna.
    expect(
      await results(ADMIN, [
        upsert('project', OPEN, 60, { name: 'Open', visibility: 'org' }),
        upsert('project', SECRET, 60, { name: 'Secret', visibility: 'members', managerId: ANNA, memberIds: [ANNA, MANAGER] }),
      ]),
    ).toEqual([{ status: 'applied' }, { status: 'applied' }])
    const projects = async (who: string) => (await pulledIds(who)).filter((r) => r === SECRET || r === OPEN)
    expect(await projects(ANNA)).toEqual([SECRET, OPEN])
    expect(await projects(MANAGER)).toEqual([SECRET, OPEN])
    expect(await projects(ADMIN)).toEqual([SECRET, OPEN])
    expect(await projects(OWNER)).toEqual([SECRET, OPEN])
    expect(await projects(BEN)).toEqual([OPEN])
    // A tag of the hidden project stays hidden with it.
    const TAG = id(33)
    expect(await results(ADMIN, [upsert('tag', TAG, 61, { name: 'internal', projectId: SECRET })])).toEqual([{ status: 'applied' }])
    expect(await pulledIds(BEN)).not.toContain(TAG)
    expect(await pulledIds(ANNA)).toContain(TAG)
  })

  it('lets a project\'s manager run it, but not hand it over', async () => {
    const SECRET = id(30)
    expect(await results(ANNA, [upsert('project', SECRET, 70, { memberIds: [ANNA, MANAGER, BEN] })])).toEqual([{ status: 'applied' }])
    expect(await results(ANNA, [upsert('project', SECRET, 71, { name: 'Renamed', visibility: 'org' })])).toEqual([{ status: 'applied' }])
    expect(await results(ANNA, [upsert('project', SECRET, 72, { managerId: BEN })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
    // Members who do not manage it, and strangers to it, may not change it; managers can only change their own projects.
    expect(await results(MANAGER, [upsert('project', SECRET, 73, { name: 'Mine now' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    expect(await results(ANNA, [upsert('project', id(32), 74, { name: 'New', managerId: ANNA })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
  })

  describe('personal rows in the shared workspace', () => {
    const E_ANNA = id(20), E_BEN = id(21)

    it('are written by their owner only', async () => {
      expect(await results(ANNA, [upsert('time_entry', E_ANNA, 10, { userId: ANNA, startAt: 1, endAt: 2 })])).toEqual([{ status: 'applied' }])
      expect(await results(BEN, [upsert('time_entry', E_BEN, 10, { userId: BEN, startAt: 3, endAt: 4 })])).toEqual([{ status: 'applied' }])
      // Not as somebody else, not on somebody else's row, not by moving a row to another owner.
      expect(await results(BEN, [upsert('time_entry', id(22), 10, { userId: ANNA, startAt: 1, endAt: 2 })])).toEqual([{ status: 'applied' }].map(() => ({ status: 'rejected', reason: 'forbidden' })))
      expect(await results(BEN, [upsert('time_entry', E_ANNA, 20, { note: 'mine now' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(ANNA, [upsert('time_entry', E_ANNA, 20, { userId: BEN })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await results(ANNA, [upsert('time_entry', id(23), 10, { startAt: 1 })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    })

    it('are corrected by managers of their people, owners and admins (entry.write_locked)', async () => {
      expect(await results(MANAGER, [upsert('time_entry', E_ANNA, 30, { note: 'fixed' })])).toEqual([{ status: 'applied' }])
      expect(await results(MANAGER, [upsert('time_entry', E_BEN, 30, { note: 'not my team' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(OWNER, [upsert('time_entry', E_BEN, 30, { note: 'fixed' })])).toEqual([{ status: 'applied' }])
      expect(await results(ADMIN, [upsert('time_entry', E_BEN, 40, { note: 'admin' })])).toEqual([{ status: 'applied' }])
      expect(await results(ANNA, [upsert('time_entry', E_BEN, 50, { note: 'peer' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      // Only time entries: other people's absences stay theirs.
      expect(await results(OWNER, [upsert('absence', id(24), 10, { userId: ANNA, startDate: '2026-01-01' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    })

    it('are seen as far as the role reaches', async () => {
      const shared = [id(10), id(11), id(12), id(30), id(31), id(33)]
      const rows = async (who: string) => (await pulledIds(who)).filter((r) => !shared.includes(r))
      expect(await rows(ANNA)).toEqual([E_ANNA])
      expect(await rows(BEN)).toEqual([E_BEN])
      expect(await rows(OWNER)).toEqual([E_ANNA, E_BEN].sort())
      // A manager sees their own department, an admin only their own data (permissions.yaml).
      expect(await rows(MANAGER)).toEqual([E_ANNA])
      expect(await rows(ADMIN)).toEqual([])
    })
  })

  describe('timesheet approval', () => {
    const SUB = id(40), E_IN = id(41), E_OUT = id(42)
    // 5 to 11 October 2026 in Vienna; entries are made on Anna's two devices.
    const monday = Date.UTC(2026, 9, 5, 7) // 09:00 Vienna
    const nextWeek = Date.UTC(2026, 9, 12, 7)
    const submission = { userId: ANNA, periodStart: '2026-10-05', periodEnd: '2026-10-11', tz: 'Europe/Vienna', status: 'submitted', note: '', submittedAt: 1, decidedBy: null, decidedAt: null, decisionNote: '' }

    it('is handed in by its owner, who cannot decide it', async () => {
      expect(await results(ANNA, [upsert('time_entry', E_IN, 100, { userId: ANNA, startAt: monday, endAt: monday + 3_600_000, locked: false })])).toEqual([{ status: 'applied' }])
      expect(await results(ANNA, [upsert('time_entry', E_OUT, 100, { userId: ANNA, startAt: nextWeek, endAt: nextWeek + 3_600_000, locked: false })])).toEqual([{ status: 'applied' }])
      expect(await results(ANNA, [upsert('timesheet_submission', SUB, 110, submission)])).toEqual([{ status: 'applied' }])
      // Nobody but its owner hands in a timesheet for them, and nobody hands it in as approved.
      expect(await results(BEN, [upsert('timesheet_submission', id(43), 110, { ...submission, userId: ANNA })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(ANNA, [upsert('timesheet_submission', id(44), 110, { ...submission, status: 'approved' })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await results(ANNA, [upsert('timesheet_submission', SUB, 120, { status: 'approved', decidedBy: ANNA })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
    })

    it('is seen by those who may approve it and by nobody else', async () => {
      const subs = async (who: string) => (await pulledIds(who)).filter((r) => r === SUB)
      for (const who of [ANNA, MANAGER, OWNER, ADMIN]) expect(await subs(who), who).toEqual([SUB])
      expect(await subs(BEN)).toEqual([])
    })

    it('is decided by someone with the right over the owner', async () => {
      const decide = (who: string, fields: Record<string, unknown>, h = 130) => results(who, [upsert('timesheet_submission', SUB, h, fields)])
      expect(await decide(BEN, { status: 'approved', decidedBy: BEN })).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      // The manager covers Anna (same department) but may change nothing but the decision.
      expect(await decide(MANAGER, { status: 'approved', decidedBy: MANAGER, periodEnd: '2026-12-31' })).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await decide(MANAGER, { status: 'approved', decidedBy: ANNA })).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await decide(MANAGER, { status: 'approved', decidedBy: MANAGER, decidedAt: 5, decisionNote: 'ok' })).toEqual([{ status: 'applied' }])
      // Decided: closed for everyone.
      expect(await decide(OWNER, { status: 'approved', decidedBy: OWNER }, 140)).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await decide(ANNA, { status: 'submitted' }, 150)).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    })

    it('locks the entries of the approved period, and only those', async () => {
      const pulled = (await app.inject({ url: '/v1/sync/pull', headers: await headers(ANNA) })).json().rows as {
        rowId: string
        fields: Record<string, { v: unknown }>
      }[]
      const locked = (rowId: string) => pulled.find((r) => r.rowId === rowId)!.fields.locked?.v
      expect(locked(E_IN)).toBe(true)
      expect(locked(E_OUT)).toBe(false)
      // Anna can no longer change a locked entry, a manager can.
      expect(await results(ANNA, [upsert('time_entry', E_IN, 200, { note: 'late' })])).toEqual([{ status: 'rejected', reason: 'locked' }])
      expect(await results(MANAGER, [upsert('time_entry', E_IN, 200, { note: 'fixed' })])).toEqual([{ status: 'applied' }])
    })

    it('can be reopened by an approver, which unlocks the period again', async () => {
      // Anna may not reopen her own approved timesheet; the owner may.
      expect(await results(ANNA, [upsert('timesheet_submission', SUB, 250, { status: 'rejected', decidedBy: ANNA })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(OWNER, [upsert('timesheet_submission', SUB, 260, { status: 'rejected', decidedBy: OWNER, decisionNote: 'reopened' })])).toEqual([{ status: 'applied' }])
      const pulled = (await app.inject({ url: '/v1/sync/pull', headers: await headers(ANNA) })).json().rows as { rowId: string; fields: Record<string, { v: unknown }> }[]
      expect(pulled.find((r) => r.rowId === E_IN)!.fields.locked?.v).toBe(false)
      // Anna can hand it in again.
      expect(await results(ANNA, [upsert('timesheet_submission', SUB, 270, { ...submission, status: 'submitted', decidedBy: null, decidedAt: null, decisionNote: '' })])).toEqual([{ status: 'applied' }])
      expect(await results(MANAGER, [upsert('timesheet_submission', SUB, 280, { status: 'approved', decidedBy: MANAGER })])).toEqual([{ status: 'applied' }])
    })

    it('can be rejected, taken back while waiting and handed in again', async () => {
      const S2 = id(45)
      const sub2 = { ...submission, periodStart: '2026-10-12', periodEnd: '2026-10-18' }
      expect(await results(ANNA, [upsert('timesheet_submission', S2, 300, sub2)])).toEqual([{ status: 'applied' }])
      expect(await results(MANAGER, [upsert('timesheet_submission', S2, 310, { status: 'rejected', decidedBy: MANAGER, decisionNote: 'missing hours' })])).toEqual([{ status: 'applied' }])
      // Handed in again: the decision is cleared.
      expect(await results(ANNA, [upsert('timesheet_submission', S2, 320, { ...sub2, status: 'submitted', decidedBy: null, decidedAt: null, decisionNote: '' })])).toEqual([{ status: 'applied' }])
      // The entries stay open while no one approved.
      const pulled = (await app.inject({ url: '/v1/sync/pull', headers: await headers(ANNA) })).json().rows as { rowId: string; fields: Record<string, { v: unknown }> }[]
      expect(pulled.find((r) => r.rowId === E_OUT)!.fields.locked?.v).toBe(false)
      expect(await results(ANNA, [{ table: 'timesheet_submission', rowId: S2, op: 'delete', hlc: hlc(330) }])).toEqual([{ status: 'applied' }])
      // An approved one cannot be taken back.
      expect(await results(ANNA, [{ table: 'timesheet_submission', rowId: SUB, op: 'delete', hlc: hlc(330) }])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    })
  })

  describe('absence requests', () => {
    const A1 = id(50)
    const request = { userId: ANNA, typeId: id(51), startDate: '2026-11-02', endDate: '2026-11-03', halfDay: null, note: '', status: 'requested', decidedBy: null, decisionNote: '' }

    it('are made by their owner, always as requests', async () => {
      expect(await results(ANNA, [upsert('absence', A1, 400, request)])).toEqual([{ status: 'applied' }])
      expect(await results(ANNA, [upsert('absence', id(52), 400, { ...request, status: 'approved' })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await results(ANNA, [upsert('absence', id(53), 400, { ...request, startDate: '2026-12-01' })].map((c) => ({ ...c, fields: { ...c.fields, status: undefined } })))).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await results(ANNA, [upsert('absence', A1, 410, { status: 'approved', decidedBy: ANNA })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      // Nobody makes a request for someone else.
      expect(await results(BEN, [upsert('absence', id(54), 400, request)])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    })

    it('are decided by someone with the right over the owner', async () => {
      const decide = (who: string, fields: Record<string, unknown>, h = 420) => results(who, [upsert('absence', A1, h, fields)])
      expect(await decide(BEN, { status: 'approved', decidedBy: BEN })).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await decide(MANAGER, { status: 'approved', decidedBy: MANAGER, endDate: '2026-12-31' })).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      expect(await decide(MANAGER, { status: 'rejected', decidedBy: MANAGER, decisionNote: 'busy' })).toEqual([{ status: 'applied' }])
      // Decided: nobody else decides it again.
      expect(await decide(OWNER, { status: 'approved', decidedBy: OWNER }, 430)).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    })

    it('ask again when a decided request is changed', async () => {
      // Changing the dates while leaving the decision in place is refused ...
      expect(await results(ANNA, [upsert('absence', A1, 440, { endDate: '2026-11-10' })])).toEqual([{ status: 'rejected', reason: 'forbidden_field' }])
      // ... changing them as a new request is fine, and waits for a decision again.
      expect(await results(ANNA, [upsert('absence', A1, 450, { endDate: '2026-11-10', status: 'requested', decidedBy: null, decisionNote: '' })])).toEqual([{ status: 'applied' }])
      expect(await results(OWNER, [upsert('absence', A1, 460, { status: 'approved', decidedBy: OWNER, decisionNote: 'ok' })])).toEqual([{ status: 'applied' }])
    })
  })

  it('shares where people work with everyone, and lets only the owner set it', async () => {
    const L = id(60)
    const loc = { userId: ANNA, date: '2026-10-05', kind: 'office' }
    expect(await results(ANNA, [upsert('work_location', L, 500, loc)])).toEqual([{ status: 'applied' }])
    expect(await results(BEN, [upsert('work_location', L, 510, { kind: 'home' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    expect(await results(OWNER, [upsert('work_location', id(61), 510, { ...loc, userId: ANNA })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    // Benn and the admin, who see none of Anna's entries, see where she works.
    for (const who of [BEN, ADMIN, MANAGER]) expect(await pulledIds(who)).toContain(L)
    expect(await results(ANNA, [upsert('work_location', L, 520, { kind: 'home' })])).toEqual([{ status: 'applied' }])
    // Offices are shared data like projects: set up by those who manage policies.
    const OFFICE = id(62)
    expect(await results(ANNA, [upsert('office', OFFICE, 530, { name: 'Vienna', capacity: 20 })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
    expect(await results(ADMIN, [upsert('office', OFFICE, 530, { name: 'Vienna', capacity: 20 })])).toEqual([{ status: 'applied' }])
    expect(await pulledIds(BEN)).toContain(OFFICE)
  })

  describe('policies', () => {
    const POL = id(70), POL_DEV = id(71)
    const E1 = id(72), E2 = id(73)
    const entry = (userId: string, extra: Record<string, unknown> = {}) => ({ userId, startAt: 1_000, endAt: 2_000, note: 'secret', projectId: id(10), tagIds: [id(11)], activityId: id(12), locked: false, ...extra })

    it('are set by people with the right and read by everyone', async () => {
      expect(await results(ANNA, [upsert('org_policy', POL, 600, { groupPath: null, requireProject: true })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(ADMIN, [upsert('org_policy', POL, 600, { groupPath: null, requireProject: true, managerVisibility: 'full' })])).toEqual([{ status: 'applied' }])
      expect(await pulledIds(BEN)).toContain(POL)
    })

    it('refuse a finished entry without what the person must give', async () => {
      // Required project (organization) and tag (the department Anna and the manager are in).
      expect(await results(ADMIN, [upsert('org_policy', POL_DEV, 610, { groupPath: 'dev', requireTags: true })])).toEqual([{ status: 'applied' }])
      expect(await results(BEN, [upsert('time_entry', id(74), 620, entry(BEN, { projectId: null }))])).toEqual([{ status: 'rejected', reason: 'policy_project' }])
      expect(await results(BEN, [upsert('time_entry', id(74), 620, entry(BEN, { tagIds: [] }))])).toEqual([{ status: 'applied' }]) // Ben is in no department
      expect(await results(ANNA, [upsert('time_entry', E1, 620, entry(ANNA, { tagIds: [] }))])).toEqual([{ status: 'rejected', reason: 'policy_tags' }])
      expect(await results(ANNA, [upsert('time_entry', E1, 621, entry(ANNA))])).toEqual([{ status: 'applied' }])
      // A running entry (no end) is not finished yet.
      expect(await results(ANNA, [upsert('time_entry', E2, 622, entry(ANNA, { endAt: null, projectId: null, tagIds: [] }))])).toEqual([{ status: 'applied' }])
    })

    it('limit what managers see of their people', async () => {
      const seen = async (who: string) => {
        const rows = (await app.inject({ url: '/v1/sync/pull', headers: await headers(who) })).json().rows as { rowId: string; fields: Record<string, { v: unknown }> }[]
        return rows.find((r) => r.rowId === E1)?.fields
      }
      expect((await seen(ANNA))!.note!.v).toBe('secret')
      expect((await seen(MANAGER))!.note!.v).toBe('secret')
      await results(ADMIN, [upsert('org_policy', POL_DEV, 630, { managerVisibility: 'hide_notes' })])
      expect((await seen(MANAGER))!.note!.v).toBe('')
      expect((await seen(MANAGER))!.projectId!.v).toBe(id(10))
      expect((await seen(ANNA))!.note!.v, 'the person still sees their own').toBe('secret')
      await results(ADMIN, [upsert('org_policy', POL_DEV, 640, { managerVisibility: 'totals_only' })])
      const totals = (await seen(MANAGER))!
      expect([totals.note!.v, totals.projectId!.v, totals.tagIds!.v]).toEqual(['', null, []])
      expect(totals.startAt!.v).toBe(1_000)
    })

    it('can hide who is online', async () => {
      // Presence is in memory; report as Anna with a connected device.
      const socket = { readyState: 1, OPEN: 1, send: () => {} }
      const remove = app.hub.add(ANNA, { socket: socket as never, deviceId: null })
      const put = async (sub: string) => app.inject({ method: 'PUT', url: `/v1/orgs/${ORG}/presence`, headers: await headers(sub), payload: { state: 'online' } })
      const list = async (sub: string) => ((await app.inject({ url: `/v1/orgs/${ORG}/presence`, headers: await headers(sub) })).json() as { userId: string }[]).map((p) => p.userId)
      await put(ANNA)
      expect(await list(BEN)).toEqual([ANNA])
      await results(ADMIN, [upsert('org_policy', POL_DEV, 650, { presenceVisibility: 'off' })])
      expect(await list(BEN)).toEqual([])
      expect(await list(ANNA)).toEqual([ANNA])
      remove()
    })
  })

  it('does not change personal workspaces', async () => {
    const mine = id(300)
    expect((await app.inject({ method: 'POST', url: '/v1/workspaces', headers: await headers(ANNA), payload: { id: mine, name: 'Anna' } })).statusCode).toBe(200)
    const res = await app.inject({
      method: 'POST',
      url: '/v1/sync/push',
      headers: await headers(ANNA),
      payload: { workspaceId: mine, deviceId: DEVICE, changes: [upsert('display_settings', id(14), 10, { theme: 'dark' }), upsert('project', id(15), 10, { name: 'Mine' })] },
    })
    expect(res.json().results).toEqual([{ status: 'applied' }, { status: 'applied' }])
  })
})
