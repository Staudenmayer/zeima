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

  describe('meetings', () => {
    const M = id(80), M2 = id(81), R_ANNA = id(82), R_BEN = id(83)
    const start = Date.UTC(2026, 10, 9, 9) // Monday 9 Nov 2026, 09:00 UTC
    const meeting = { userId: MANAGER, title: 'Sync', startAt: start, endAt: start + 3_600_000, tz: 'UTC', note: '', rrule: 'FREQ=WEEKLY;COUNT=2', attendeeIds: [ANNA], cancelled: false }
    const answer = (fields: object) => ({ meetingId: M, status: 'accepted', ...fields })

    it('are made by their organizer and seen only by those asked', async () => {
      expect(await results(MANAGER, [upsert('meeting', M, 700, meeting)])).toEqual([{ status: 'applied' }])
      expect(await results(ANNA, [upsert('meeting', M, 710, { title: 'Mine now' })])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      for (const who of [MANAGER, ANNA]) expect(await pulledIds(who), who).toContain(M)
      // Not even the owner and the admin see a meeting they are not asked to.
      for (const who of [BEN, ADMIN, OWNER]) expect(await pulledIds(who), who).not.toContain(M)
    })

    it('are answered by the people asked, visible to those asked', async () => {
      expect(await results(ANNA, [upsert('meeting_response', R_ANNA, 720, answer({ userId: ANNA }))])).toEqual([{ status: 'applied' }])
      // Not by the organizer, not by someone who is not asked, not for someone else.
      expect(await results(MANAGER, [upsert('meeting_response', id(84), 720, answer({ userId: MANAGER }))])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(BEN, [upsert('meeting_response', R_BEN, 720, answer({ userId: BEN }))])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      expect(await results(BEN, [upsert('meeting_response', id(85), 720, answer({ userId: ANNA }))])).toEqual([{ status: 'rejected', reason: 'forbidden' }])
      for (const who of [MANAGER, ANNA]) expect(await pulledIds(who), who).toContain(R_ANNA)
      expect(await pulledIds(BEN)).not.toContain(R_ANNA)
    })

    it('tell others when someone is busy, and nothing more', async () => {
      const ask = async (who: string, body: object) =>
        app.inject({ method: 'POST', url: `/v1/orgs/${ORG}/freebusy`, headers: await headers(who), payload: body })
      const week = { from: start - 3_600_000, to: start + 20 * 86_400_000 }
      const res = await ask(BEN, { userIds: [ANNA, MANAGER, BEN], ...week })
      expect(res.statusCode).toBe(200)
      const busy = res.json() as Record<string, { from: number; to: number; kind: string }[]>
      // Two Mondays for the organizer and for Anna, who accepted; Ben is free.
      expect(busy[ANNA]!.filter((b) => b.kind === 'meeting').map((b) => b.from)).toEqual([start, start + 7 * 86_400_000])
      expect(busy[MANAGER]!.filter((b) => b.kind === 'meeting')).toHaveLength(2)
      expect(busy[BEN]).toEqual([])
      expect(JSON.stringify(busy)).not.toContain('Sync')
      // Declining frees the person; a cancelled meeting is nobody's.
      await results(ANNA, [upsert('meeting_response', R_ANNA, 730, { status: 'declined' })])
      const meetingsOf = async (who: string) => ((await ask(BEN, { userIds: [who], ...week })).json()[who] as { kind: string }[]).filter((b) => b.kind === 'meeting')
      expect(await meetingsOf(ANNA)).toEqual([])
      await results(MANAGER, [upsert('meeting', M2, 740, { ...meeting, startAt: start + 86_400_000, endAt: start + 86_400_000 + 3_600_000, rrule: null, attendeeIds: [], cancelled: true })])
      expect(await meetingsOf(MANAGER)).toHaveLength(2)
      // Approved absences are busy days; requests are not.
      await results(ANNA, [upsert('absence', id(86), 750, { userId: ANNA, typeId: id(51), startDate: '2026-11-11', endDate: '2026-11-12', halfDay: null, note: '', status: 'requested', decidedBy: null, decisionNote: '' })])
      const absenceDays = async () => ((await ask(BEN, { userIds: [ANNA], ...week })).json()[ANNA] as { from: number; to: number; kind: string }[]).filter((b) => b.kind === 'absence' && b.from === Date.UTC(2026, 10, 11))
      expect(await absenceDays()).toEqual([])
      await results(MANAGER, [upsert('absence', id(86), 760, { status: 'approved', decidedBy: MANAGER })])
      expect(await absenceDays()).toEqual([{ from: Date.UTC(2026, 10, 11), to: Date.UTC(2026, 10, 13), kind: 'absence' }])
      // Limits: a sane window, and members only.
      expect((await ask(BEN, { userIds: [ANNA], from: 0, to: 400 * 86_400_000 })).statusCode).toBe(400)
      expect((await app.inject({ method: 'POST', url: `/v1/orgs/${ORG}/freebusy`, headers: await headers(OUTSIDER), payload: { userIds: [ANNA], ...week } })).statusCode).toBe(403)
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
