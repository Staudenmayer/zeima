import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { schema } from '../src/db/index.ts'
import http from 'node:http'
import { CHALLENGE_NAME, checkImportedAddresses, checkSourceUrl, fetchMetadata, normalizeDomains } from '../src/modules/orgs/sso.ts'
import { FakeKeycloak } from './fake-keycloak.ts'
import { keys, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const OWNER = id(1), ADMIN = id(2), MEMBER = id(4), STRANGER = id(5)
const ORG = id(900)

const PUBLIC = async () => [{ address: '93.184.216.34' }]
const OIDC = { protocol: 'oidc', name: 'Acme sign-in', sourceUrl: 'https://login.acme.example/tenant/v2.0', clientId: 'zeima', clientSecret: 's3cret', domains: ['Acme.example'] }

describe('the address of a provider', () => {
  it('must be a public https address', async () => {
    const bad = (url: string, resolver = PUBLIC) => expect(checkSourceUrl(url, false, resolver)).rejects.toMatchObject({ status: 400 })
    await bad('not a url')
    await bad('http://login.acme.example/x')
    await bad('https://user:pw@login.acme.example/x')
    await bad('https://localhost/x')
    await bad('https://127.0.0.1/x')
    await bad('https://10.0.0.5/x')
    await bad('https://[::1]/x')
    await bad('https://intranet/x')
    await bad('https://idp.corp.internal/x')
    await bad('https://sneaky.example/x', async () => [{ address: '10.1.2.3' }])
    await bad('https://sneaky6.example/x', async () => [{ address: 'fd00::1' }])
    await bad('https://nowhere.example/x', async () => {
      throw new Error('ENOTFOUND')
    })
    await expect(checkSourceUrl('https://login.microsoftonline.com/t/v2.0', false, PUBLIC)).resolves.toBeInstanceOf(URL)
  })

  it('may be on a private network when the server allows it (development)', async () => {
    await expect(checkSourceUrl('http://localhost:8080/realms/x', true)).resolves.toBeInstanceOf(URL)
    await expect(checkSourceUrl('ftp://localhost/x', true)).rejects.toMatchObject({ status: 400 })
  })

  it('has email domains that are tidied up and checked', () => {
    expect(normalizeDomains([' @Acme.Example ', 'acme.example', 'sub.acme.co.uk'])).toEqual(['acme.example', 'sub.acme.co.uk'])
    for (const bad of ['acme', 'ac me.example', '-x.example', 'a..b.example', 'x.example/path']) {
      expect(() => normalizeDomains([bad]), bad).toThrow()
    }
  })
})

describe('reading a provider\'s metadata', () => {
  let server: http.Server
  let port = 0
  const seen: string[] = []
  beforeAll(async () => {
    server = http.createServer((req, res) => {
      seen.push(`${req.headers.host} ${req.url}`)
      if (req.url === '/redirect') return void res.writeHead(302, { location: 'http://169.254.169.254/latest' }).end()
      if (req.url === '/huge') return void res.writeHead(200).end(Buffer.alloc(2 * 1024 * 1024, 97))
      res.writeHead(200).end('{"ok":true}')
    })
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r))
    port = (server.address() as { port: number }).port
  })
  afterAll(() => new Promise<void>((r) => server.close(() => r())))

  it('connects to the address that was checked, not to a second answer of the name (DNS rebinding)', async () => {
    let lookups = 0
    // The name answers with a private address the first time and a public one later: only the first counts.
    const rebinding = async () => (lookups++ === 0 ? [{ address: '127.0.0.1' }] : [{ address: '93.184.216.34' }])
    const text = await fetchMetadata(new URL(`http://rebind.example:${port}/doc`), true, rebinding)
    expect(text).toBe('{"ok":true}')
    expect(lookups).toBe(1)
    expect(seen.at(-1)).toBe(`rebind.example:${port} /doc`)
  })

  it('refuses a name that resolves to a private address when private ones are not allowed', async () => {
    await expect(fetchMetadata(new URL(`http://evil.example:${port}/doc`), false, async () => [{ address: '10.0.0.5' }])).rejects.toMatchObject({ status: 400 })
    await expect(fetchMetadata(new URL(`http://evil.example:${port}/doc`), false, async () => [{ address: '169.254.169.254' }])).rejects.toMatchObject({ status: 400 })
    await expect(fetchMetadata(new URL(`http://evil.example:${port}/doc`), false, async () => [{ address: '127.0.0.1' }])).rejects.toMatchObject({ status: 400 })
  })

  it('does not follow redirects and stops reading a huge answer', async () => {
    const resolver = async () => [{ address: '127.0.0.1' }]
    await expect(fetchMetadata(new URL(`http://x.example:${port}/redirect`), true, resolver)).rejects.toMatchObject({ status: 400, message: expect.stringContaining('redirects') })
    expect(seen.some((s) => s.includes('169.254'))).toBe(false)
    await expect(fetchMetadata(new URL(`http://x.example:${port}/huge`), true, resolver)).rejects.toMatchObject({ status: 400 })
  })

  it('refuses a provider whose metadata names an address on a private network', async () => {
    const public_ = async () => [{ address: '93.184.216.34' }]
    await expect(checkImportedAddresses({ tokenUrl: 'https://login.acme.example/token' }, false, public_)).resolves.toBeUndefined()
    for (const [key, value] of [['tokenUrl', 'https://127.0.0.1/token'], ['jwksUrl', 'https://169.254.169.254/keys'], ['userInfoUrl', 'https://10.0.0.1/me'], ['authorizationUrl', 'http://login.acme.example/auth'], ['tokenUrl', 'https://intranet/token'], ['singleSignOnServiceUrl', 'https://localhost/sso']] as const) {
      await expect(checkImportedAddresses({ [key]: value }, false, public_), `${key} ${value}`).rejects.toMatchObject({ status: 400 })
    }
    await expect(checkImportedAddresses({ tokenUrl: 'https://sneaky.example/token' }, false, async () => [{ address: '10.1.1.1' }])).rejects.toMatchObject({ status: 400 })
    // Development setups may use private addresses.
    await expect(checkImportedAddresses({ tokenUrl: 'http://localhost:8080/token' }, true)).resolves.toBeUndefined()
  })
})

describe.skipIf(!(await reachable()))('single sign-on of an organization', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  let kc: FakeKeycloak
  let app: Awaited<ReturnType<typeof build>>
  let strict: Awaited<ReturnType<typeof build>>
  const published = new Map<string, string[][]>()

  const build = (env: Record<string, string> = {}, extra: { dnsTxt?: (name: string) => Promise<string[][]> } = {}) =>
    buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false', ...env }), {
      logger: false,
      keys,
      readiness: {},
      db: db.db,
      keycloakAdmin: kc,
      dnsResolver: PUBLIC,
      ssoFetch: async () => '{}',
      ...extra,
    })

  const as = async (userId: string, role: string | null) => ({
    authorization: `Bearer ${await token({ sub: userId, email: `${userId.slice(-1)}@example.com`, groups: role ? [`/orgs/${ORG}/roles/${role}`] : [] })}`,
  })
  const call = async (a: typeof app, method: 'GET' | 'PUT' | 'DELETE', userId: string, role: string | null, payload?: object) =>
    a.inject({ method, url: `/v1/orgs/${ORG}/sso`, headers: await as(userId, role), payload })

  beforeAll(async () => {
    db = await createTestDatabase()
    kc = new FakeKeycloak()
    app = await build({ SSO_VERIFY_DOMAINS: 'false' })
    strict = await build({ SSO_VERIFY_DOMAINS: 'true', SSO_CHALLENGE_SECRET: 'a-test-challenge-secret' }, { dnsTxt: async (name) => published.get(name) ?? [] })
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    for (const [uid, role] of [[OWNER, 'owner'], [ADMIN, 'admin'], [MEMBER, 'member']] as const) {
      await db.db.insert(schema.kcUser).values({ id: uid, email: `${uid.slice(-1)}@example.com`, displayName: role }).onConflictDoNothing()
      await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: uid, role })
    }
  })
  afterAll(async () => {
    await app?.close()
    await strict?.close()
    await db?.drop()
  })

  it('says where to point the provider, before anything is set up', async () => {
    const res = await call(app, 'GET', OWNER, 'owner')
    expect(res.statusCode).toBe(200)
    const body = res.json()
    expect(body).toMatchObject({ configured: false, domains: [], enabled: false, domainChallenge: null })
    expect(body.redirectUri).toBe(`http://localhost:8080/realms/zeima/broker/sso-${ORG}/endpoint`)
    expect(body.samlMetadataUrl).toBe(`${body.redirectUri}/descriptor`)
  })

  it('refuses to set up a provider whose metadata points at an internal address', async () => {
    kc.metadata = { ...kc.metadata, tokenUrl: 'https://169.254.169.254/token' }
    const strictResolver = await build({ SSO_VERIFY_DOMAINS: 'false' })
    try {
      const res = await call(strictResolver, 'PUT', OWNER, 'owner', OIDC)
      expect(res.statusCode).toBe(400)
      expect(res.json().detail).toContain('tokenUrl')
      expect(kc.sso.has(ORG)).toBe(false)
    } finally {
      kc.metadata = { issuer: 'https://login.acme.example', tokenUrl: 'https://login.acme.example/token', authorizationUrl: 'https://login.acme.example/auth', jwksUrl: 'https://login.acme.example/keys' }
      await strictResolver.close()
    }
  })

  it('is for owners and admins; members and strangers are refused', async () => {
    for (const [uid, role] of [[MEMBER, 'member'], [STRANGER, null]] as const) {
      for (const method of ['GET', 'PUT', 'DELETE'] as const) {
        const res = await call(app, method, uid, role, method === 'PUT' ? OIDC : undefined)
        expect(res.statusCode, `${role} ${method}`).toBe(403)
      }
    }
    expect(kc.sso.size).toBe(0)
  })

  it('sets up an OIDC provider; the secret is never given back', async () => {
    kc.calls = []
    const res = await call(app, 'PUT', ADMIN, 'admin', OIDC)
    expect(res.statusCode).toBe(200)
    expect(res.json()).toMatchObject({ configured: true, protocol: 'oidc', name: 'Acme sign-in', clientId: 'zeima', domains: ['acme.example'], enabled: true })
    expect(JSON.stringify(res.json())).not.toContain('s3cret')
    expect(kc.calls).toEqual(["parseSsoMetadata oidc", `setSso ${ORG} oidc acme.example`])
    const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.orgId, ORG))
    expect(log.map((l) => l.action)).toContain('sso.set')
    expect(JSON.stringify(log.map((l) => l.detail))).not.toContain('s3cret')
  })

  it('changes it without the secret: the stored one stays', async () => {
    const res = await call(app, 'PUT', OWNER, 'owner', { ...OIDC, clientSecret: undefined, name: 'Acme SSO', domains: ['acme.example', 'acme.io'] })
    expect(res.statusCode).toBe(200)
    expect(res.json()).toMatchObject({ name: 'Acme SSO', domains: ['acme.example', 'acme.io'] })
    expect(kc.sso.get(ORG)?.clientSecret).toBe('s3cret')
  })

  it('needs the pieces of the kind of provider', async () => {
    await call(app, 'DELETE', OWNER, 'owner')
    expect((await call(app, 'PUT', OWNER, 'owner', { ...OIDC, clientId: undefined })).statusCode).toBe(400)
    expect((await call(app, 'PUT', OWNER, 'owner', { ...OIDC, clientSecret: undefined })).statusCode).toBe(400)
    expect((await call(app, 'PUT', OWNER, 'owner', { ...OIDC, domains: [] })).statusCode).toBe(400)
    expect((await call(app, 'PUT', OWNER, 'owner', { ...OIDC, domains: ['nope'] })).statusCode).toBe(400)
    expect((await call(app, 'PUT', OWNER, 'owner', { ...OIDC, sourceUrl: 'http://login.acme.example/x' })).statusCode).toBe(400)
    expect((await call(app, 'PUT', OWNER, 'owner', { ...OIDC, sourceUrl: 'https://192.168.1.1/x' })).statusCode).toBe(400)
    expect(kc.sso.size).toBe(0)
    // A SAML provider has no client.
    const saml = await call(app, 'PUT', OWNER, 'owner', { protocol: 'saml', name: 'Acme SAML', sourceUrl: 'https://idp.acme.example/metadata', domains: ['acme.example'] })
    expect(saml.statusCode).toBe(200)
    expect(saml.json()).toMatchObject({ protocol: 'saml', configured: true })
    expect(saml.json().clientId).toBeUndefined()
  })

  it('removes it again', async () => {
    expect((await call(app, 'PUT', OWNER, 'owner', OIDC)).statusCode).toBe(200)
    const res = await call(app, 'DELETE', OWNER, 'owner')
    expect(res.statusCode).toBe(204)
    expect((await call(app, 'GET', OWNER, 'owner')).json()).toMatchObject({ configured: false, domains: [] })
    const log = await db.db.select().from(schema.auditLog).where(eq(schema.auditLog.orgId, ORG))
    expect(log.map((l) => l.action)).toContain('sso.delete')
  })

  it('has no built-in secret for the domain proof: without one it says so instead of making a guessable value', async () => {
    const bare = await build({ SSO_VERIFY_DOMAINS: 'true' })
    try {
      const res = await call(bare, 'GET', OWNER, 'owner')
      expect(res.statusCode).toBe(503)
      expect(res.json().detail).toMatch(/secret/)
    } finally {
      await bare.close()
    }
  })

  it('makes an organization prove its domains where that is required', async () => {
    const asked = await call(strict, 'GET', OWNER, 'owner')
    const challenge = asked.json().domainChallenge
    expect(challenge).toMatchObject({ name: CHALLENGE_NAME })
    expect(challenge.value).toMatch(/^zeima-verification=[0-9a-f]{32}$/)

    const refused = await call(strict, 'PUT', OWNER, 'owner', OIDC)
    expect(refused.statusCode).toBe(400)
    expect(refused.json().code).toBe('domain_not_verified')
    expect(refused.json().detail).toContain(`${CHALLENGE_NAME}.acme.example`)
    // Somebody else's value does not do.
    published.set(`${CHALLENGE_NAME}.acme.example`, [['zeima-verification=' + '0'.repeat(32)]])
    expect((await call(strict, 'PUT', OWNER, 'owner', OIDC)).statusCode).toBe(400)
    // The organization's own does, however the record is split into strings.
    published.set(`${CHALLENGE_NAME}.acme.example`, [[challenge.value.slice(0, 20), challenge.value.slice(20)]])
    expect((await call(strict, 'PUT', OWNER, 'owner', OIDC)).statusCode).toBe(200)
    // Every domain needs it.
    expect((await call(strict, 'PUT', OWNER, 'owner', { ...OIDC, domains: ['acme.example', 'other.example'] })).statusCode).toBe(400)
  })

  it('is an enterprise feature where plans are enforced', async () => {
    const cloud = await build({ DEPLOYMENT_MODE: 'cloud', BILLING_WEBHOOK_SECRET: 'billing-secret' })
    const res = await call(cloud, 'GET', OWNER, 'owner')
    expect(res.statusCode).toBe(403)
    expect(res.json()).toMatchObject({ code: 'feature_not_available', detail: 'your plan does not include sso' })
    await cloud.close()
  })
})
