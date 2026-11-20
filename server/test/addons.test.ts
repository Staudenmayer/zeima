import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { keys, token, syncMirror } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const OWNER = id(1), MEMBER = id(4), STRANGER = id(5)
const ORG = id(900)

const folder = mkdtempSync(join(tmpdir(), 'zeima-registry-'))
const profiles = mkdtempSync(join(tmpdir(), 'zeima-profiles-'))
const ZIP = Buffer.from('PK-not-really-a-zip')
const entry = (over: object = {}) => ({ id: 'dev.example.demo', name: 'Demo', version: '1.0.0', description: 'd', author: 'a', minAppVersion: '1.0.0', permissions: ['notify'], hooks: ['schedule.tick'], file: 'demo-1.0.0.zip', sha256: 'a'.repeat(64), size: ZIP.length, keyId: 'k1', ...over })

mkdirSync(join(folder), { recursive: true })
writeFileSync(join(folder, 'demo-1.0.0.zip'), ZIP)
writeFileSync(join(folder, 'secret.zip'), 'not listed')
writeFileSync(
  join(folder, 'index.json'),
  JSON.stringify({
    version: 1,
    publishers: [{ keyId: 'k1', name: 'Example Ltd', publicKey: 'AAAA' }, { keyId: 3 }],
    addons: [entry(), entry({ id: 'x', file: 'x.zip' }), entry({ id: 'dev.example.evil', file: '../index.json' }), entry({ id: 'dev.example.sneaky', file: 'a/b.zip' })],
  }),
)
writeFileSync(join(profiles, 'AT.toml'), 'id = "AT"\n')
writeFileSync(join(profiles, 'XX.toml'), 'id = "XX"\n')
writeFileSync(join(profiles, 'notes.txt'), 'ignored')

describe.skipIf(!(await reachable()))('addons: the registry and the organization rules', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  let app: Awaited<ReturnType<typeof buildApp>>
  let empty: Awaited<ReturnType<typeof buildApp>>

  const make = (env: Record<string, string>) =>
    buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false', ...env }), { logger: false, keys, readiness: {}, db: db.db, keycloakAdmin: new FakeKeycloak() })
  const as = async (userId: string, role: string | null) => {
    const groups = role ? [`/orgs/${ORG}/roles/${role}`] : []
    await syncMirror(db.db, userId, groups)
    return { authorization: `Bearer ${await token({ sub: userId, email: `${userId.slice(-1)}@example.com`, groups })}` }
  }

  beforeAll(async () => {
    db = await createTestDatabase()
    app = await make({ ADDON_REGISTRY_DIR: folder, JURISDICTIONS_DIR: profiles })
    empty = await make({})
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
  })
  afterAll(async () => {
    await app?.close()
    await empty?.close()
    await db?.drop()
    rmSync(folder, { recursive: true, force: true })
    rmSync(profiles, { recursive: true, force: true })
  })

  describe('the registry', () => {
    it('needs a sign-in', async () => {
      expect((await app.inject({ method: 'GET', url: '/v1/addons/registry' })).statusCode).toBe(401)
      expect((await app.inject({ method: 'GET', url: '/v1/addons/registry/files/demo-1.0.0.zip' })).statusCode).toBe(401)
    })

    it('lists the addons and publishers, and leaves out what points outside the folder or is malformed', async () => {
      const res = await app.inject({ method: 'GET', url: '/v1/addons/registry', headers: await as(STRANGER, null) })
      expect(res.statusCode).toBe(200)
      const body = res.json()
      expect(body.publishers).toEqual([{ keyId: 'k1', name: 'Example Ltd', publicKey: 'AAAA' }])
      expect(body.addons.map((a: { id: string }) => a.id)).toEqual(['dev.example.demo'])
      expect(body.addons[0]).toMatchObject({ keyId: 'k1', sha256: 'a'.repeat(64), file: 'demo-1.0.0.zip' })
    })

    it('is empty without a registry folder', async () => {
      const res = await empty.inject({ method: 'GET', url: '/v1/addons/registry', headers: await as(STRANGER, null) })
      expect(res.json()).toEqual({ publishers: [], addons: [] })
    })

    it('serves a listed package and nothing else', async () => {
      const headers = await as(STRANGER, null)
      const ok = await app.inject({ method: 'GET', url: '/v1/addons/registry/files/demo-1.0.0.zip', headers })
      expect(ok.statusCode).toBe(200)
      expect(ok.headers['content-type']).toContain('application/zip')
      expect(ok.rawPayload.equals(ZIP)).toBe(true)
      for (const file of ['secret.zip', 'index.json', '..%2Findex.json', 'x.zip', 'nope.zip']) {
        expect((await app.inject({ method: 'GET', url: `/v1/addons/registry/files/${file}`, headers })).statusCode, file).toBe(404)
      }
    })
  })

  describe('the rules of an organization', () => {
    const put = async (userId: string, role: string | null, payload: object) =>
      app.inject({ method: 'PUT', url: `/v1/orgs/${ORG}/addons`, headers: await as(userId, role), payload })
    const get = async (userId: string, role: string | null) => app.inject({ method: 'GET', url: `/v1/orgs/${ORG}/addons`, headers: await as(userId, role) })

    it('allow everything until the organization says otherwise; every member can read them', async () => {
      const res = await get(MEMBER, 'member')
      expect(res.json()).toEqual({ mode: 'open', requireSigned: false, allowed: [] })
      expect((await get(STRANGER, null)).statusCode).toBe(403)
    })

    it('are changed by owners and admins only', async () => {
      const body = { mode: 'allowlist', requireSigned: true, allowed: ['dev.example.demo'] }
      expect((await put(MEMBER, 'member', body)).statusCode).toBe(403)
      expect((await put(STRANGER, null, body)).statusCode).toBe(403)
      const res = await put(OWNER, 'owner', { ...body, allowed: ['Dev.Example.Demo', 'dev.example.demo', 'com.acme.hours'] })
      expect(res.statusCode).toBe(200)
      expect(res.json()).toEqual({ mode: 'allowlist', requireSigned: true, allowed: ['com.acme.hours', 'dev.example.demo'] })
      expect((await get(MEMBER, 'member')).json().mode).toBe('allowlist')
      const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.orgId, ORG))
      expect(log.map((l) => l.action)).toContain('addons.policy')
    })

    it('refuse things that are not addon ids or modes', async () => {
      expect((await put(OWNER, 'owner', { mode: 'allowlist', requireSigned: false, allowed: ['not an id'] })).statusCode).toBe(400)
      expect((await put(OWNER, 'owner', { mode: 'sometimes', requireSigned: false, allowed: [] })).statusCode).toBe(400)
    })
  })

  describe('extra country profiles', () => {
    it('are served as text from the mounted folder, without signing in', async () => {
      const res = await app.inject({ method: 'GET', url: '/v1/jurisdictions' })
      expect(res.statusCode).toBe(200)
      expect(res.json().profiles.map((p: { file: string }) => p.file)).toEqual(['AT.toml', 'XX.toml'])
      expect(res.json().profiles[0].source).toContain('id = "AT"')
    })

    it('are none without a folder', async () => {
      expect((await empty.inject({ method: 'GET', url: '/v1/jurisdictions' })).json()).toEqual({ profiles: [] })
    })
  })
})
