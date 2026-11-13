// Entitlement tokens (SPEC §2): what a user or organization may use, signed
// with Ed25519 so the app can check it offline against a key built into it.
//
// A token is a compact JWS:  base64url(header).base64url(payload).base64url(signature)
//   header   { "alg": "EdDSA", "typ": "zeima-entitlement+jwt" }
//   payload  { v: 1, sub, orgId?, plan, seats?, features[], iat, exp, graceDays? }  (times in unix seconds)
//
// Mirrors crates/zeima-core/src/entitlement.rs; both run the vectors in
// packages/shared-fixtures/entitlement/vectors.json.

import { createPrivateKey, createPublicKey, type KeyObject, sign, verify } from 'node:crypto'

export const TOKEN_TYPE = 'zeima-entitlement+jwt'
export const DEFAULT_GRACE_DAYS = 14

export interface EntitlementClaims {
  v: 1
  /** Keycloak user id the token was issued for. */
  sub: string
  orgId?: string
  plan: string
  seats?: number
  features: string[]
  iat: number
  /** End of the paid period, unix seconds. */
  exp: number
  /** Days the features keep working after `exp`, for devices that cannot reach the server. */
  graceDays?: number
}

export type EntitlementStatus = 'valid' | 'grace' | 'expired'

export type VerifyError = 'malformed' | 'bad_header' | 'bad_payload' | 'bad_signature'

export type Verified =
  | { status: EntitlementStatus; plan: string; features: string[]; orgId?: string; seats?: number; claims: EntitlementClaims }
  | { status: 'invalid'; error: VerifyError }

const FREE = { plan: 'free', features: [] as string[] }

const b64 = (data: Buffer | string) => Buffer.from(data).toString('base64url')
const unb64 = (text: string) => Buffer.from(text, 'base64url')

/** A public key from its 32 raw bytes in base64url. */
export function publicKeyFromRaw(raw: string): KeyObject {
  return createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: raw }, format: 'jwk' })
}

/** A private key from PKCS#8 DER in base64url. */
export function privateKeyFromPkcs8(der: string): KeyObject {
  return createPrivateKey({ key: unb64(der), format: 'der', type: 'pkcs8' })
}

export function signEntitlement(claims: EntitlementClaims, key: KeyObject): string {
  const signing = `${b64(JSON.stringify({ alg: 'EdDSA', typ: TOKEN_TYPE }))}.${b64(JSON.stringify(claims))}`
  return `${signing}.${b64(sign(null, Buffer.from(signing), key))}`
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(unb64(text).toString('utf8'))
  } catch {
    return undefined
  }
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

function claimsOf(payload: unknown): EntitlementClaims | null {
  if (!isObject(payload) || payload.v !== 1) return null
  const { sub, plan, features, iat, exp, orgId, seats, graceDays } = payload
  if (typeof sub !== 'string' || sub === '' || typeof plan !== 'string' || plan === '') return null
  if (!Array.isArray(features) || !features.every((f) => typeof f === 'string')) return null
  if (typeof iat !== 'number' || typeof exp !== 'number') return null
  if (orgId !== undefined && typeof orgId !== 'string') return null
  if (seats !== undefined && typeof seats !== 'number') return null
  if (graceDays !== undefined && typeof graceDays !== 'number') return null
  return { v: 1, sub, plan, features: features as string[], iat, exp, orgId, seats, graceDays }
}

/** Checks the signature against the trusted keys and works out what the token allows at `now` (unix seconds). */
export function verifyEntitlement(token: string, trusted: readonly KeyObject[], now: number): Verified {
  const parts = token.split('.')
  if (parts.length !== 3) return { status: 'invalid', error: 'malformed' }
  const [h, p, s] = parts as [string, string, string]
  const header = parseJson(h)
  if (!isObject(header) || header.alg !== 'EdDSA' || header.typ !== TOKEN_TYPE) return { status: 'invalid', error: 'bad_header' }
  const signature = unb64(s)
  const ok = trusted.some((key) => verify(null, Buffer.from(`${h}.${p}`), key, signature))
  if (!ok) return { status: 'invalid', error: 'bad_signature' }
  const claims = claimsOf(parseJson(p))
  if (!claims) return { status: 'invalid', error: 'bad_payload' }

  const graceEnd = claims.exp + (claims.graceDays ?? 0) * 86400
  if (now > graceEnd) return { status: 'expired', ...FREE, claims }
  return {
    status: now > claims.exp ? 'grace' : 'valid',
    plan: claims.plan,
    features: claims.features,
    orgId: claims.orgId,
    seats: claims.seats,
    claims,
  }
}

/** Features each plan includes (SPEC §2). Plans can be repackaged here; clients only look at features. */
export const PLAN_FEATURES: Record<string, readonly string[]> = {
  free: [],
  pro: ['sync'],
  team: ['sync', 'org', 'presence', 'approvals', 'shared_data'],
  enterprise: ['sync', 'org', 'presence', 'approvals', 'shared_data', 'sso'],
}

/** Plan strength, to pick the best of several entitlements. */
export const PLAN_RANK = ['free', 'pro', 'team', 'enterprise']
