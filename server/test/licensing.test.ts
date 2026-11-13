import { readFileSync } from 'node:fs'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { privateKeyFromPkcs8, publicKeyFromRaw, signEntitlement, verifyEntitlement } from '../src/licensing/entitlement.ts'
import { createTestDatabase, reachable } from './pg.ts'
import { keys, token } from './helpers.ts'

const dev = JSON.parse(readFileSync(new URL('../../packages/shared-fixtures/entitlement/dev-key.json', import.meta.url), 'utf8')) as {
  publicKey: string
  privateKeyPkcs8: string
}
const trusted = [publicKeyFromRaw(dev.publicKey)]

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const ADA = id(1), WS = id(100), ORG = id(500)
const hlc = (wall: number) => `${String(wall).padStart(15, '0')}-000000-a`

describe.skipIf(!(await reachable()))('entitlements', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  beforeAll(async () => (db = await createTestDatabase()))
  afterAll(async () => db?.drop())

  const appWith = (env: Record<string, string> = {}) =>
    buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false', ...env }), { logger: false, keys, readiness: {}, db: db.db })
  const get = async (app: Awaited<ReturnType<typeof appWith>>) =>
    app.inject({ url: '/v1/entitlement', headers: { authorization: `Bearer ${await token({ sub: ADA, groups: [`/orgs/${ORG}/roles/member`] })}` } })
  const now = () => Math.floor(Date.now() / 1000)

  it('is Free without an entitlement, and has no token', async () => {
    const app = await appWith()
    const res = await get(app)
    expect(res.statusCode).toBe(200)
    expect(res.json()).toEqual({ plan: 'free', features: [], status: 'valid', orgId: null, seats: null, expiresAt: null, token: null })
    await app.close()
  })

  it('requires sign-in', async () => {
    const app = await appWith()
    expect((await app.inject('/v1/entitlement')).statusCode).toBe(401)
    await app.close()
  })

  it('signs a token for the default plan (development)', async () => {
    const app = await appWith({ DEFAULT_PLAN: 'pro' })
    const body = (await get(app)).json()
    expect(body.plan).toBe('pro')
    expect(body.features).toEqual(['sync'])
    const verified = verifyEntitlement(body.token, trusted, now())
    expect(verified.status).toBe('valid')
    expect((verified as { claims: { sub: string } }).claims.sub).toBe(ADA)
    await app.close()
  })

  it('uses the best of the user\'s and their organization\'s entitlements', async () => {
    await db.db.insert(schema.kcUser).values({ id: ADA, displayName: 'Ada' }).onConflictDoNothing()
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Org', createdAt: 1 })
    await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: ADA, role: 'member' })
    await db.db.insert(schema.entitlement).values([
      { id: id(601), userId: ADA, plan: 'pro', updatedAt: 1 },
      { id: id(602), orgId: ORG, plan: 'team', seats: 25, updatedAt: 1, validUntil: Date.now() + 86_400_000 },
    ])
    const app = await appWith()
    const body = (await get(app)).json()
    expect(body.plan).toBe('team')
    expect(body.seats).toBe(25)
    expect(body.orgId).toBe(ORG)
    expect(body.features).toContain('presence')
    await app.close()
  })

  it('passes a vendor license through as it is, and ignores one that does not verify', async () => {
    await db.db.delete(schema.entitlement)
    const license = signEntitlement(
      { v: 1, sub: ADA, orgId: ORG, plan: 'enterprise', seats: 100, features: ['sync', 'sso'], iat: now(), exp: now() + 86_400 * 365 },
      privateKeyFromPkcs8(dev.privateKeyPkcs8),
    )
    await db.db.insert(schema.entitlement).values({ id: id(603), orgId: ORG, plan: 'enterprise', token: license, updatedAt: 1 })
    const app = await appWith()
    const body = (await get(app)).json()
    expect(body.token).toBe(license)
    expect(body.plan).toBe('enterprise')
    expect(body.seats).toBe(100)

    await db.db.delete(schema.entitlement)
    await db.db.insert(schema.entitlement).values({ id: id(604), orgId: ORG, plan: 'enterprise', token: license.slice(0, -4) + 'AAAA', updatedAt: 1 })
    expect((await get(app)).json().plan).toBe('free')
    await app.close()
  })

  it('lets lapsed entitlements fall back to Free', async () => {
    await db.db.delete(schema.entitlement)
    await db.db.insert(schema.entitlement).values({ id: id(605), userId: ADA, plan: 'pro', validUntil: Date.now() - 1000, updatedAt: 1 })
    const app = await appWith()
    expect((await get(app)).json().plan).toBe('free')
    await app.close()
  })

  describe('cloud enforcement', () => {
    const push = async (app: Awaited<ReturnType<typeof appWith>>) =>
      app.inject({
        method: 'POST',
        url: '/v1/sync/push',
        headers: { authorization: `Bearer ${await token({ sub: ADA })}` },
        payload: { workspaceId: WS, deviceId: id(900), changes: [{ table: 'project', rowId: id(700), op: 'upsert', hlc: hlc(10), fields: { name: 'x' } }] },
      })

    it('refuses sync on the Free plan', async () => {
      await db.db.delete(schema.entitlement)
      const app = await appWith({ DEPLOYMENT_MODE: 'cloud' })
      const res = await push(app)
      expect(res.statusCode).toBe(403)
      expect(res.json().code).toBe('feature_not_available')
      await app.close()
    })

    it('allows sync on Pro', async () => {
      await db.db.insert(schema.entitlement).values({ id: id(606), userId: ADA, plan: 'pro', updatedAt: 1 })
      const app = await appWith({ DEPLOYMENT_MODE: 'cloud' })
      // Passes the plan check; there is no such workspace, which is a different error.
      expect((await push(app)).statusCode).toBe(404)
      await app.close()
    })

    it('does not enforce on self-hosted servers', async () => {
      await db.db.delete(schema.entitlement)
      const app = await appWith()
      expect((await push(app)).statusCode).toBe(404)
      await app.close()
    })
  })

  describe('license import (self-hosted)', () => {
    const adminHeaders = async () => ({ authorization: `Bearer ${await token({ sub: ADA, realm_access: { roles: ['user', 'platform-admin'] } })}` })
    const userHeaders = async () => ({ authorization: `Bearer ${await token({ sub: ADA })}` })
    const license = (claims: Partial<Parameters<typeof signEntitlement>[0]> = {}) =>
      signEntitlement(
        { v: 1, sub: ADA, plan: 'pro', features: ['sync'], iat: now(), exp: now() + 86_400 * 365, graceDays: 14, ...claims },
        privateKeyFromPkcs8(dev.privateKeyPkcs8),
      )
    const put = async (app: Awaited<ReturnType<typeof appWith>>, tokenText: string, headers?: Record<string, string>) =>
      app.inject({ method: 'PUT', url: '/v1/license', headers: headers ?? (await adminHeaders()), payload: { token: tokenText } })

    it('is for administrators only', async () => {
      const app = await appWith()
      expect((await put(app, license(), await userHeaders())).statusCode).toBe(403)
      expect((await app.inject({ url: '/v1/license', headers: await userHeaders() })).statusCode).toBe(403)
      await app.close()
    })

    it('installs a signed license, which the user\'s app then receives as it is', async () => {
      await db.db.delete(schema.entitlement)
      const app = await appWith()
      const token = license()
      const res = await put(app, token)
      expect(res.statusCode).toBe(200)
      expect(res.json()).toMatchObject({ plan: 'pro', userId: ADA, orgId: null, status: 'valid' })

      expect((await get(app)).json().token).toBe(token)
      const listed = (await app.inject({ url: '/v1/license', headers: await adminHeaders() })).json()
      expect(listed).toHaveLength(1)

      // Installing a new one for the same subject replaces it.
      const newer = license({ plan: 'team', features: ['sync', 'org'] })
      expect((await put(app, newer)).statusCode).toBe(200)
      expect((await app.inject({ url: '/v1/license', headers: await adminHeaders() })).json()).toHaveLength(1)
      expect((await get(app)).json().plan).toBe('team')
      await app.close()
    })

    it('refuses licenses that are forged, unsigned or expired', async () => {
      const app = await appWith()
      const good = license()
      expect((await put(app, `${good.slice(0, -4)}AAAA`)).statusCode).toBe(400)
      expect((await put(app, 'this is not a license at all')).statusCode).toBe(400)
      expect((await put(app, license({ exp: now() - 86_400 * 60, iat: now() - 86_400 * 90 }))).statusCode).toBe(400)
      await app.close()
    })

    it('is not available on the cloud', async () => {
      const app = await appWith({ DEPLOYMENT_MODE: 'cloud' })
      expect((await put(app, license())).statusCode).toBe(404)
      await app.close()
    })
  })
})
