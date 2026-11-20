// A token names organizations for minutes; the mirror of Keycloak's organizations is what counts (membership, role).

import { and, eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { ensureOrgWorkspace } from '../src/modules/orgs/service.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { keys, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const OWNER = id(1), ADMIN = id(2), LEAVER = id(3)
const ORG = id(900), DEVICE = id(901)
const hlc = (wall: number) => `${String(wall).padStart(15, '0')}-000000-a`

describe.skipIf(!(await reachable()))('membership is taken from the database, not the token', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  let kc: FakeKeycloak
  // `development`, not `test`: the per-person cache of memberships is on, as in production.
  let app: Awaited<ReturnType<typeof buildApp>>
  let ws: string

  const as = async (sub: string, role: string, extra: Record<string, string> = {}) => ({
    authorization: `Bearer ${await token({ sub, groups: [`/orgs/${ORG}/roles/${role}`] })}`,
    ...extra,
  })
  const member = async (sub: string, role: string) => {
    await db.db.insert(schema.kcUser).values({ id: sub, email: `${sub.slice(-1)}@example.com`, displayName: `u${sub.slice(-1)}` }).onConflictDoNothing()
    await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: sub, role }).onConflictDoUpdate({ target: [schema.kcOrgMember.orgId, schema.kcOrgMember.userId], set: { role } })
  }
  const push = async (sub: string, role: string, changes: unknown[]) =>
    app.inject({ method: 'POST', url: '/v1/sync/push', headers: await as(sub, role), payload: { workspaceId: ws, deviceId: DEVICE, changes } })
  const entry = (rowId: string, userId: string) => ({
    table: 'pause',
    rowId,
    op: 'upsert',
    hlc: hlc(10),
    fields: { userId, startAt: 1_800_000_000_000, endAt: 1_800_000_060_000 },
  })

  beforeAll(async () => {
    db = await createTestDatabase()
    kc = new FakeKeycloak()
    app = await buildApp(loadConfig({ NODE_ENV: 'development', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false' }), { logger: false, keys, readiness: {}, db: db.db, keycloakAdmin: kc })
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    ws = await ensureOrgWorkspace(db.db, ORG, 'Acme')
  })
  afterAll(async () => {
    await app?.close()
    await db?.drop()
  })

  it('refuses a person who is no longer in the mirror, wherever the token still names the organization', async () => {
    const { forgetMembership } = await import('../src/modules/orgs/roles.ts')
    await member(OWNER, 'owner')
    await member(LEAVER, 'member')
    forgetMembership(LEAVER)
    const members = async () => app.inject({ url: `/v1/orgs/${ORG}/members`, headers: await as(LEAVER, 'member') })
    expect((await members()).statusCode).toBe(200)
    await db.db.delete(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, ORG), eq(schema.kcOrgMember.userId, LEAVER)))
    forgetMembership(LEAVER)
    expect((await members()).statusCode).toBe(403)
    const header = await app.inject({ url: '/v1/me', headers: await as(LEAVER, 'member', { 'x-org-id': ORG }) })
    expect(header.statusCode).toBe(403)
    expect(header.json().code).toBe('org_forbidden')
    expect((await push(LEAVER, 'member', [entry(id(10), LEAVER)])).statusCode).toBeGreaterThanOrEqual(400)
  })

  it('takes the role from the mirror: a token that still says admin does not grant admin rights', async () => {
    const { forgetMembership } = await import('../src/modules/orgs/roles.ts')
    await member(ADMIN, 'admin')
    forgetMembership(ADMIN)
    const invitations = async () => app.inject({ url: `/v1/orgs/${ORG}/invitations`, headers: await as(ADMIN, 'admin') })
    expect((await invitations()).statusCode).toBe(200)
    await member(ADMIN, 'member')
    forgetMembership(ADMIN)
    expect((await invitations()).statusCode).toBe(403)
  })

  it('does not let a removed member push rows that stay behind, even while their access is cached', async () => {
    const { forgetMembership } = await import('../src/modules/orgs/roles.ts')
    await member(OWNER, 'owner')
    await member(LEAVER, 'member')
    forgetMembership(LEAVER)
    const mine = id(20)
    expect((await push(LEAVER, 'member', [entry(mine, LEAVER)])).json().results).toEqual([{ status: 'applied' }])
    // The access is now cached. Removing the member by the API ends it at once...
    expect((await app.inject({ method: 'DELETE', url: `/v1/orgs/${ORG}/members/${LEAVER}`, headers: await as(OWNER, 'owner') })).statusCode).toBe(204)
    const after = await push(LEAVER, 'member', [entry(id(21), LEAVER)])
    expect(after.statusCode).toBeGreaterThanOrEqual(400)
    // ...and a removal that did not reach this process's cache (another replica) is stopped by the check in the push itself.
    await member(LEAVER, 'member')
    forgetMembership(LEAVER)
    expect((await push(LEAVER, 'member', [entry(id(22), LEAVER)])).statusCode).toBe(200)
    await db.db.delete(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, ORG), eq(schema.kcOrgMember.userId, LEAVER)))
    const late = await push(LEAVER, 'member', [entry(id(23), LEAVER)])
    expect(late.statusCode).toBe(403)
    const rows = await db.db.select().from(schema.syncRow).where(eq(schema.syncRow.workspaceId, ws))
    // Nothing of the removed member's pushes after the removal (or of those before it) is left.
    expect(rows.filter((r) => [id(20), id(21), id(23)].includes(r.rowId))).toEqual([])
  })
})
