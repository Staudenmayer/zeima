// Row-level security against a real PostgreSQL (the dev stack:
// `docker compose -f deploy/compose.dev.yml up -d postgres`). Skipped when no
// database is reachable. Each run uses its own throwaway database.

import { sql } from 'drizzle-orm'
import postgres from 'postgres'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { testApp } from './helpers.ts'
import { connect, type Db, enableTenantLogin, migrateDb, schema, tenantUrlFor, withUser } from '../src/db/index.ts'

const ADMIN_URL = process.env.TEST_DATABASE_URL ?? 'postgres://zeima:zeima@localhost:5432/postgres'
const NAME = `zeima_test_${process.pid}_${Date.now()}`

const admin = postgres(ADMIN_URL, { max: 1, connect_timeout: 2, onnotice: () => {} })
const reachable = await admin`select 1`.then(() => true, () => false)

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const ORG_A = id(100), ORG_B = id(200)
const ADA = id(1), BOB = id(2), EVE = id(3)
const WS_ADA = id(11), WS_A = id(12), WS_B = id(13)
const DEVICE = id(900)
const now = 1_700_000_000_000
const row = { createdAt: now, updatedAt: now, hlc: '0', originDevice: DEVICE }

describe.skipIf(!reachable)('row-level security', () => {
  let db: Db
  let close: () => Promise<unknown>

  beforeAll(async () => {
    await admin.unsafe(`create database ${NAME}`)
    const url = new URL(ADMIN_URL)
    url.pathname = `/${NAME}`
    const conn = connect(url.toString(), { onnotice: () => {} })
    db = conn.db
    close = () => conn.client.end()
    await migrateDb(db)

    // Seeded as the owner, the way the Keycloak mirror writes it.
    await db.insert(schema.organization).values([
      { id: ORG_A, name: 'A', createdAt: now },
      { id: ORG_B, name: 'B', createdAt: now },
    ])
    await db.insert(schema.kcUser).values([
      { id: ADA, displayName: 'Ada' },
      { id: BOB, displayName: 'Bob' },
      { id: EVE, displayName: 'Eve' },
    ])
    await db.insert(schema.kcOrgMember).values([
      { orgId: ORG_A, userId: ADA, role: 'member' },
      { orgId: ORG_A, userId: BOB, role: 'manager' },
      { orgId: ORG_B, userId: EVE, role: 'owner' },
    ])
    await db.insert(schema.workspace).values([
      { id: WS_ADA, kind: 'personal', ownerId: ADA, name: 'Ada personal', ...row },
      { id: WS_A, kind: 'org', orgId: ORG_A, name: 'A', ...row },
      { id: WS_B, kind: 'org', orgId: ORG_B, name: 'B', ...row },
    ])
    await db.insert(schema.project).values([
      { id: id(21), workspaceId: WS_ADA, name: 'Private', color: '#000', ...row },
      { id: id(22), workspaceId: WS_A, name: 'Shared A', color: '#111', ...row },
      { id: id(23), workspaceId: WS_B, name: 'Shared B', color: '#222', ...row },
    ])
    await db.insert(schema.device).values({ id: DEVICE, userId: ADA, name: 'laptop', platform: 'linux', createdAt: now })
  })

  afterAll(async () => {
    await close?.()
    await admin.unsafe(`drop database if exists ${NAME} with (force)`).catch(() => {})
  })

  /** Drizzle wraps driver errors; the Postgres message is in `cause`. */
async function denied(p: Promise<unknown>, re: RegExp) {
  const err = await p.then(() => null, (e: Error & { cause?: Error }) => e)
  expect(err, 'expected the query to be refused').not.toBeNull()
  expect(`${err?.message} ${err?.cause?.message ?? ''}`).toMatch(re)
}

  const names = (rows: { name: string }[]) => rows.map((r) => r.name).sort()
  const as = (userId: string, ...orgIds: string[]) => ({ userId, orgIds })

  it('shows a user their personal workspace and their orgs\' workspaces only', async () => {
    const ada = await withUser(db, as(ADA, ORG_A), (tx) => tx.select().from(schema.project))
    expect(names(ada)).toEqual(['Private', 'Shared A'])
    const bob = await withUser(db, as(BOB, ORG_A), (tx) => tx.select().from(schema.project))
    expect(names(bob)).toEqual(['Shared A'])
    const eve = await withUser(db, as(EVE, ORG_B), (tx) => tx.select().from(schema.project))
    expect(names(eve)).toEqual(['Shared B'])
  })

  it('shows nothing without an identity', async () => {
    const rows = await withUser(db, as('00000000-0000-4000-8000-000000000999'), (tx) => tx.select().from(schema.project))
    expect(rows).toEqual([])
  })

  it('refuses writes into another tenant\'s workspace', async () => {
    const insert = withUser(db, as(EVE, ORG_B), (tx) =>
      tx.insert(schema.project).values({ id: id(24), workspaceId: WS_A, name: 'Sneaky', color: '#333', ...row }),
    )
    await denied(insert, /row-level security/)
  })

  it('does not let updates or deletes reach other tenants\' rows', async () => {
    await withUser(db, as(EVE, ORG_B), async (tx) => {
      await tx.update(schema.project).set({ name: 'Hijacked' }).where(sql`true`)
      await tx.delete(schema.project).where(sql`name = 'Shared A'`)
    })
    const rows = await db.select().from(schema.project)
    expect(names(rows)).toEqual(['Private', 'Shared A', 'Hijacked'].sort())
    expect(rows.find((r) => r.id === id(22))?.name).toBe('Shared A')
  })

  it('limits identity data to the caller\'s orgs', async () => {
    const orgs = await withUser(db, as(ADA, ORG_A), (tx) => tx.select().from(schema.organization))
    expect(names(orgs)).toEqual(['A'])
    const users = await withUser(db, as(ADA, ORG_A), (tx) => tx.select().from(schema.kcUser))
    expect(users.map((u) => u.displayName).sort()).toEqual(['Ada', 'Bob'])
  })

  it('makes identity data read-only for requests', async () => {
    const write = withUser(db, as(ADA, ORG_A), (tx) => tx.update(schema.kcUser).set({ displayName: 'Mallory' }))
    await denied(write, /permission denied/)
  })

  it('keeps devices private and the audit log per org', async () => {
    expect(await withUser(db, as(ADA, ORG_A), (tx) => tx.select().from(schema.device))).toHaveLength(1)
    expect(await withUser(db, as(BOB, ORG_A), (tx) => tx.select().from(schema.device))).toHaveLength(0)

    await withUser(db, as(BOB, ORG_A), (tx) =>
      tx.insert(schema.auditLog).values({ orgId: ORG_A, actorId: BOB, action: 'member.invite', at: now }),
    )
    const foreign = withUser(db, as(BOB, ORG_A), (tx) =>
      tx.insert(schema.auditLog).values({ orgId: ORG_B, actorId: BOB, action: 'x', at: now }),
    )
    await denied(foreign, /row-level security/)
    expect(await withUser(db, as(EVE, ORG_B), (tx) => tx.select().from(schema.auditLog))).toHaveLength(0)
    expect(await withUser(db, as(BOB, ORG_A), (tx) => tx.select().from(schema.auditLog))).toHaveLength(1)
  })

  it('does not leak the identity into later queries on the same connection', async () => {
    await withUser(db, as(ADA, ORG_A), (tx) => tx.select().from(schema.project))
    const ownerView = await db.select().from(schema.project)
    expect(ownerView).toHaveLength(3)
  })

  it('reports the database in /readyz', async () => {
    const app = await testApp({ db })
    const ok = await app.inject('/readyz')
    expect(ok.json()).toEqual({ status: 'ok', checks: { database: 'ok' } })
    await close()
    const down = await app.inject('/readyz')
    expect(down.statusCode).toBe(503)
    expect(down.json().checks.database).toBe('error')
    await app.close()
  })
})

describe.skipIf(!reachable)('requests through a restricted login', () => {
  const NAME2 = `zeima_test_tenant_${process.pid}_${Date.now()}`
  const PASSWORD = 'a-test-password-for-zeima_app'
  let owner: ReturnType<typeof connect>

  beforeAll(async () => {
    await admin.unsafe(`create database ${NAME2}`)
    const url = new URL(ADMIN_URL)
    url.pathname = `/${NAME2}`
    const ownerOnly = connect(url.toString(), { onnotice: () => {} })
    await migrateDb(ownerOnly.db)
    await enableTenantLogin(ownerOnly.db, PASSWORD)
    await ownerOnly.client.end()
    owner = connect(url.toString(), { onnotice: () => {} }, tenantUrlFor(url.toString(), PASSWORD))
    await owner.db.insert(schema.kcUser).values([{ id: ADA, email: 'ada@example.com' }, { id: BOB, email: 'bob@example.com' }])
    await owner.db.insert(schema.workspace).values([
      { id: WS_ADA, kind: 'personal', ownerId: ADA, name: 'Ada', ...row },
      { id: WS_B, kind: 'personal', ownerId: BOB, name: 'Bob', ...row },
    ])
  })
  afterAll(async () => {
    await owner?.tenantClient?.end()
    await owner?.client.end()
    await admin.unsafe(`drop database if exists ${NAME2} with (force)`).catch(() => {})
    await admin.end()
  })

  it('runs as the restricted role, which is no superuser and has no way back to the owner', async () => {
    await withUser(owner.db, { userId: ADA, orgIds: [] }, async (tx) => {
      const [me] = (await tx.execute(sql`select current_user as u, (select rolsuper from pg_roles where rolname = current_user) as su, (select rolbypassrls from pg_roles where rolname = current_user) as bypass`)) as unknown as { u: string; su: boolean; bypass: boolean }[]
      expect(me).toEqual({ u: 'zeima_app', su: false, bypass: false })
    })
    // The session cannot become the owner (what a stolen statement in `withUser` could otherwise do).
    await expect(
      withUser(owner.db, { userId: ADA, orgIds: [] }, async (tx) => {
        await tx.execute(sql`reset role`)
        await tx.execute(sql`set role zeima`)
        return tx.select().from(schema.workspace)
      }),
    ).rejects.toThrow()
  })

  it('still shows each person only their own rows', async () => {
    const mine = await withUser(owner.db, { userId: ADA, orgIds: [] }, (tx) => tx.select().from(schema.workspace))
    expect(mine.map((w) => w.id)).toEqual([WS_ADA])
    const bobs = await withUser(owner.db, { userId: BOB, orgIds: [] }, (tx) => tx.select().from(schema.workspace))
    expect(bobs.map((w) => w.id)).toEqual([WS_B])
  })

  it('refuses a password that could break out of the statement', async () => {
    await expect(enableTenantLogin(owner.db, "x'; drop table kc_user; --")).rejects.toThrow(/16 to 128/)
    await expect(enableTenantLogin(owner.db, 'short')).rejects.toThrow()
  })
})
