// Works out what a signed-in user may use and issues the token for it
// (SPEC §2). Entitlements come from the `entitlement` table: rows for the
// user and for the organizations they belong to; the best plan wins. A row
// may carry a vendor-signed license token (self-hosted); that is served as it
// is. Otherwise, with a signing key, the server signs one.

import { readFileSync } from 'node:fs'
import type { KeyObject } from 'node:crypto'
import { inArray, or, eq } from 'drizzle-orm'
import type { Config } from '../config.ts'
import { callerOf, type Db, schema, withUser } from '../db/index.ts'
import type { AuthUser } from '../plugins/auth.ts'
import { HttpProblem } from '../plugins/errors.ts'
import {
  DEFAULT_GRACE_DAYS,
  type EntitlementStatus,
  PLAN_FEATURES,
  PLAN_RANK,
  privateKeyFromPkcs8,
  publicKeyFromRaw,
  signEntitlement,
  verifyEntitlement,
} from './entitlement.ts'

/** A token lives this long when the entitlement has no end date; devices refresh it on every sync. */
const OPEN_ENDED_DAYS = 30

export interface Issued {
  plan: string
  features: string[]
  status: EntitlementStatus
  orgId: string | null
  seats: number | null
  /** End of the paid period, unix seconds; null for Free. */
  expiresAt: number | null
  /** Signed token for the app to keep; null for Free. */
  token: string | null
}

const FREE: Issued = { plan: 'free', features: [], status: 'valid', orgId: null, seats: null, expiresAt: null, token: null }

/** The development key, used outside production when none is configured. It is public on purpose. */
function developmentKey() {
  const file = new URL('../../../packages/shared-fixtures/entitlement/dev-key.json', import.meta.url)
  return JSON.parse(readFileSync(file, 'utf8')) as { publicKey: string; privateKeyPkcs8: string }
}

export class Licensing {
  private readonly signingKey: KeyObject | null
  private readonly trusted: KeyObject[]
  private readonly defaultPlan: string
  private readonly cloud: boolean

  constructor(config: Config) {
    const dev = config.env === 'production' ? null : developmentKey()
    const signing = config.licensing.signingKey ?? dev?.privateKeyPkcs8 ?? null
    this.signingKey = signing ? privateKeyFromPkcs8(signing) : null
    this.trusted = [...config.licensing.publicKeys, ...(dev ? [dev.publicKey] : [])].map(publicKeyFromRaw)
    this.defaultPlan = config.licensing.defaultPlan
    this.cloud = config.deploymentMode === 'cloud'
  }

  /** Checks a license token against the trusted vendor keys. */
  verifyLicense(token: string, now = Date.now()) {
    return verifyEntitlement(token, this.trusted, Math.floor(now / 1000))
  }

  /** Cloud deployments enforce plans on the API; self-hosted ones rely on the license the app checks. */
  get enforced() {
    return this.cloud
  }

  async resolve(db: Db, user: AuthUser, now = Date.now()): Promise<Issued> {
    const orgIds = Object.keys(user.orgs)
    const rows = await withUser(db, callerOf(user), (tx) =>
      tx
        .select()
        .from(schema.entitlement)
        .where(orgIds.length ? or(eq(schema.entitlement.userId, user.id), inArray(schema.entitlement.orgId, orgIds)) : eq(schema.entitlement.userId, user.id)),
    )
    const nowSeconds = Math.floor(now / 1000)

    // Candidates, best plan first; a license token is only usable if it verifies.
    const candidates = rows
      .filter((r) => r.validUntil === null || r.validUntil > now || r.token !== null)
      .sort((a, b) => PLAN_RANK.indexOf(b.plan) - PLAN_RANK.indexOf(a.plan))
    for (const row of candidates) {
      if (row.token) {
        const verified = verifyEntitlement(row.token, this.trusted, nowSeconds)
        if (verified.status === 'invalid') continue
        return {
          plan: verified.plan,
          features: verified.features,
          status: verified.status,
          orgId: verified.orgId ?? null,
          seats: verified.seats ?? null,
          expiresAt: verified.claims.exp,
          token: row.token,
        }
      }
      const features = Array.isArray(row.features) && row.features.length ? (row.features as string[]) : [...(PLAN_FEATURES[row.plan] ?? [])]
      return this.issue(user, { plan: row.plan, features, orgId: row.orgId, seats: row.seats, until: row.validUntil }, nowSeconds)
    }

    if (this.defaultPlan !== 'free') {
      return this.issue(user, { plan: this.defaultPlan, features: [...(PLAN_FEATURES[this.defaultPlan] ?? [])], orgId: null, seats: null, until: null }, nowSeconds)
    }
    return FREE
  }

  private issue(
    user: AuthUser,
    e: { plan: string; features: string[]; orgId: string | null; seats: number | null; until: number | null },
    nowSeconds: number,
  ): Issued {
    if (!this.signingKey) throw new HttpProblem(503, 'licensing_unavailable', 'this server cannot sign entitlements')
    const exp = e.until !== null ? Math.floor(e.until / 1000) : nowSeconds + OPEN_ENDED_DAYS * 86400
    const token = signEntitlement(
      {
        v: 1,
        sub: user.id,
        ...(e.orgId && { orgId: e.orgId }),
        plan: e.plan,
        ...(e.seats !== null && { seats: e.seats }),
        features: e.features,
        iat: nowSeconds,
        exp,
        graceDays: DEFAULT_GRACE_DAYS,
      },
      this.signingKey,
    )
    return { plan: e.plan, features: e.features, status: 'valid', orgId: e.orgId, seats: e.seats, expiresAt: exp, token }
  }

  /** Throws unless the user's plan includes `feature` (cloud only). */
  async require(db: Db, user: AuthUser, feature: string): Promise<void> {
    if (!this.cloud) return
    const issued = await this.resolve(db, user)
    if (!issued.features.includes(feature)) {
      throw new HttpProblem(403, 'feature_not_available', `your plan does not include ${feature}`)
    }
  }
}
