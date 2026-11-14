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
      const shared = [id(10), id(11), id(12)]
      const rows = async (who: string) => (await pulledIds(who)).filter((r) => !shared.includes(r))
      expect(await rows(ANNA)).toEqual([E_ANNA])
      expect(await rows(BEN)).toEqual([E_BEN])
      expect(await rows(OWNER)).toEqual([E_ANNA, E_BEN].sort())
      // A manager sees their own department, an admin only their own data (permissions.yaml).
      expect(await rows(MANAGER)).toEqual([E_ANNA])
      expect(await rows(ADMIN)).toEqual([])
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
