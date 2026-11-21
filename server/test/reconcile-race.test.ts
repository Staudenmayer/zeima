import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { schema } from '../src/db/index.ts'
import { ensureOrgWorkspace } from '../src/modules/orgs/service.ts'
import { reconcileOrganization } from '../src/modules/orgs/reconcile.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`

describe.skipIf(!(await reachable()))('a member who joins while the organization is being compared with Keycloak', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  beforeAll(async () => {
    db = await createTestDatabase()
  })
  afterAll(async () => {
    await db?.drop()
  })

  it('is left alone (not removed or purged); a real leaver in the same run still is', async () => {
    const org = id(900)
    const [owner, stay, leaver, joiner, newcomer] = [id(1), id(2), id(3), id(4), id(5)]
    const u = (userId: string, n: number) => ({ id: userId, email: `${n}@race.example`, name: `u${n}`, enabled: true })
    await db.db.insert(schema.organization).values({ id: org, name: 'Race', createdAt: 1 })
    const ws = await ensureOrgWorkspace(db.db, org, 'Race')
    for (const [n, userId] of [owner, stay, leaver, joiner, newcomer].entries()) {
      await db.db.insert(schema.kcUser).values({ id: userId, email: `${n + 1}@race.example`, displayName: `u${n + 1}` }).onConflictDoNothing()
    }
    for (const [userId, role] of [[owner, 'owner'], [stay, 'member'], [leaver, 'member']] as const) await db.db.insert(schema.kcOrgMember).values({ orgId: org, userId, role })
    for (const [k, userId] of [leaver, joiner].entries()) {
      await db.db.insert(schema.syncRow).values({ workspaceId: ws, tableName: 'time_entry', rowId: id(800 + k), fields: { userId: { v: userId, h: 'h' } } })
    }
    // Keycloak lists the owner, one who stays and a newcomer beyond the seats (turned away, which calls Keycloak after
    // the snapshot was read): inside that call an invitation is accepted / a SCIM user is created, without the owner lock.
    const kc = new FakeKeycloak()
    kc.members = [u(owner, 1), u(stay, 2), u(newcomer, 5)]
    kc.roles = { owner: [owner], admin: [], manager: [], member: [stay] }
    kc.removeMember = async () => {
      await db.db.insert(schema.kcOrgMember).values({ orgId: org, userId: joiner, role: 'member' })
    }
    const result = await reconcileOrganization(db.db, kc, org, { seatLimit: async () => 1 })
    expect(result.rejectedMembers).toBe(1)
    const members = (await db.db.select().from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, org))).map((m) => m.userId).sort()
    expect(members).toEqual([owner, stay, joiner].sort())
    const sync = (await db.db.select().from(schema.syncRow).where(eq(schema.syncRow.workspaceId, ws))).map((r) => (r.fields as { userId: { v: string } }).userId.v)
    expect(sync).toEqual([joiner])
  })
})
