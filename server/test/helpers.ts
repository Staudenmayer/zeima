// Test app with local signing keys instead of Keycloak's JWKS.

import { randomUUID } from 'node:crypto'
import { eq } from 'drizzle-orm'
import { createLocalJWKSet, exportJWK, generateKeyPair, type JWTPayload, SignJWT } from 'jose'
import { type AppOptions, buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'
import { type Db, schema } from '../src/db/index.ts'

export const config = loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false' })

const { privateKey, publicKey } = await generateKeyPair('RS256')
const jwk = { ...(await exportJWK(publicKey)), kid: 'test-key', alg: 'RS256', use: 'sig' }
export const keys = createLocalJWKSet({ keys: [jwk] })

type SigningKey = Parameters<SignJWT['sign']>[0]

/** A Keycloak-like access token; `claims` override the defaults. */
export async function token(claims: JWTPayload = {}, options: { expiresIn?: string | number; key?: SigningKey } = {}) {
  return new SignJWT({
    iss: config.keycloak.issuer,
    aud: ['zeima-api', 'account'],
    sub: '2c8f1f5e-0000-4000-8000-000000000001',
    typ: 'Bearer',
    azp: 'zeima-desktop',
    email: 'ada@example.com',
    name: 'Ada Lovelace',
    locale: 'de',
    realm_access: { roles: ['user', 'offline_access'] },
    groups: [],
    ...claims,
  })
    .setProtectedHeader({ alg: 'RS256', kid: 'test-key', typ: 'JWT' })
    .setIssuedAt()
    .setExpirationTime(options.expiresIn ?? '5m')
    .sign(options.key ?? privateKey)
}

export async function testApp(options: AppOptions = {}) {
  return buildApp(config, { logger: false, keys, readiness: {}, ...options })
}

/**
 * Makes the database mirror of Keycloak's organizations agree with the groups of a token: the API takes membership, role
 * and departments from the mirror, so a test that mints a token for an organization member needs the member in it too.
 * Existing rows are changed to the token's role; nothing is removed.
 */
export async function syncMirror(db: Db, sub: string, groups: readonly string[]) {
  for (const path of groups) {
    const [, root, orgId, kind, ...rest] = path.split('/')
    if (root !== 'orgs' || !orgId || !kind) continue
    const [org] = await db.select({ id: schema.organization.id }).from(schema.organization).where(eq(schema.organization.id, orgId))
    if (!org) continue
    await db.insert(schema.kcUser).values({ id: sub, email: `${sub}@example.test` }).onConflictDoNothing()
    if (kind === 'roles' && rest.length === 1) {
      await db.insert(schema.kcOrgMember).values({ orgId, userId: sub, role: rest[0]! }).onConflictDoUpdate({ target: [schema.kcOrgMember.orgId, schema.kcOrgMember.userId], set: { role: rest[0]! } })
    } else if (kind === 'groups') {
      await db.insert(schema.kcOrgMember).values({ orgId, userId: sub, role: 'member' }).onConflictDoNothing()
      const [group] = await db.insert(schema.kcGroup).values({ id: randomUUID(), orgId, path }).onConflictDoUpdate({ target: schema.kcGroup.path, set: { orgId } }).returning({ id: schema.kcGroup.id })
      await db.insert(schema.kcGroupMember).values({ groupId: group!.id, userId: sub }).onConflictDoNothing()
    }
  }
}

/**
 * One API replica against a shared test database with the PostgreSQL bus on. `development` rather than `test`, so the
 * per-person membership cache is on as in production. Several of these in one test make a multi-replica deployment.
 */
export async function replica(database: { db: Db; url: string }, options: AppOptions = {}, env: Record<string, string> = {}) {
  const cfg = loadConfig({ NODE_ENV: 'development', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false', BUS: 'pg', DATABASE_URL: database.url, RATE_LIMIT_PER_MINUTE: '100000', BUS_RESYNC_SPREAD_MS: '0', BUS_RESYNC_MIN_INTERVAL_MS: '0', ...env })
  return buildApp(cfg, { logger: false, keys, readiness: {}, db: database.db, ...options })
}
