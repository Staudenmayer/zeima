// Test app with local signing keys instead of Keycloak's JWKS.

import { createLocalJWKSet, exportJWK, generateKeyPair, type JWTPayload, SignJWT } from 'jose'
import { type AppOptions, buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'

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
