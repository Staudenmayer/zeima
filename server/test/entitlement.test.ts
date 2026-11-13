// The shared entitlement vectors (packages/shared-fixtures/entitlement/vectors.json);
// the Rust verifier runs the same file.

import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { privateKeyFromPkcs8, publicKeyFromRaw, signEntitlement, verifyEntitlement } from '../src/licensing/entitlement.ts'

const dir = new URL('../../packages/shared-fixtures/entitlement/', import.meta.url)
const vectors = JSON.parse(readFileSync(new URL('vectors.json', dir), 'utf8')) as {
  now: number
  trustedKeys: string[]
  cases: { name: string; token: string; expect: Record<string, unknown> }[]
}
const devKey = JSON.parse(readFileSync(new URL('dev-key.json', dir), 'utf8')) as { publicKey: string; privateKeyPkcs8: string }
const trusted = vectors.trustedKeys.map(publicKeyFromRaw)

describe('entitlement vectors', () => {
  for (const c of vectors.cases) {
    it(c.name, () => {
      const result = verifyEntitlement(c.token, trusted, vectors.now)
      const { claims: _claims, ...seen } = result as typeof result & { claims?: unknown }
      // Only the keys the vector states are compared; optional ones may be undefined.
      for (const [key, value] of Object.entries(c.expect)) expect(seen[key as keyof typeof seen], key).toEqual(value)
    })
  }
})

describe('signing', () => {
  it('produces tokens the verifier accepts, with the development key', () => {
    expect(devKey.publicKey).toBe(vectors.trustedKeys[0])
    const token = signEntitlement(
      { v: 1, sub: 'u1', plan: 'pro', features: ['sync'], iat: vectors.now, exp: vectors.now + 100 },
      privateKeyFromPkcs8(devKey.privateKeyPkcs8),
    )
    const result = verifyEntitlement(token, trusted, vectors.now)
    expect(result.status).toBe('valid')
  })
})
