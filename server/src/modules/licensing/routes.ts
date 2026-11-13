// The caller's entitlement (SPEC §2): the plan, its features and a signed
// token the app keeps and checks offline.

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { eq, isNotNull } from 'drizzle-orm'
import { Type } from 'typebox'
import { schema } from '../../db/index.ts'
import { uuidFor } from '../billing/lemonsqueezy.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'

const Entitlement = Type.Object(
  {
    plan: Type.String(),
    features: Type.Array(Type.String()),
    status: Type.Union([Type.Literal('valid'), Type.Literal('grace'), Type.Literal('expired')]),
    orgId: Type.Union([Type.String(), Type.Null()]),
    seats: Type.Union([Type.Integer(), Type.Null()]),
    expiresAt: Type.Union([Type.Integer(), Type.Null()], { description: 'End of the paid period, unix seconds' }),
    token: Type.Union([Type.String(), Type.Null()], { description: 'Ed25519-signed token for offline checks; null on the Free plan' }),
  },
  { description: 'What the caller may use' },
)

export const licensingRoutes: FastifyPluginAsyncTypebox = async (app) => {
  app.get(
    '/entitlement',
    {
      schema: {
        tags: ['licensing'],
        summary: 'The caller\'s plan, features and signed entitlement token',
        security: SECURITY,
        response: { 200: Entitlement, ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
      return app.licensing.resolve(app.db, request.user!)
    },
  )
}

const License = Type.Object(
  {
    plan: Type.String(),
    features: Type.Array(Type.String()),
    status: Type.Union([Type.Literal('valid'), Type.Literal('grace'), Type.Literal('expired')]),
    orgId: Type.Union([Type.String(), Type.Null()]),
    userId: Type.Union([Type.String(), Type.Null()]),
    seats: Type.Union([Type.Integer(), Type.Null()]),
    expiresAt: Type.Integer({ description: 'End of the paid period, unix seconds' }),
  },
  { description: 'A license held by this server' },
)

/** Realm role of the people who run the installation. */
const ADMIN_ROLE = 'platform-admin'

// Offline license files for self-hosted installations (SPEC §2, §7.1): the
// vendor signs an entitlement token, an administrator uploads it here, and
// the server hands it to the apps of the user or organization it names. Cloud
// servers get their entitlements from billing instead.
export const licenseRoutes: FastifyPluginAsyncTypebox = async (app) => {
  const database = () => {
    if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
    return app.db
  }
  const requireAdmin = (roles: readonly string[]) => {
    if (!roles.includes(ADMIN_ROLE)) throw new HttpProblem(403, 'forbidden', `this needs the ${ADMIN_ROLE} role`)
  }

  app.put(
    '/license',
    {
      schema: {
        tags: ['licensing'],
        summary: 'Install a license file (self-hosted)',
        description: 'The token must be signed by a trusted issuer (`ENTITLEMENT_PUBLIC_KEYS`). It replaces the license for the same user or organization.',
        security: SECURITY,
        body: Type.Object({ token: Type.String({ minLength: 20, maxLength: 20_000 }) }),
        response: { 200: License, ...problems(400, 401, 403, 503) },
      },
    },
    async (request) => {
      requireAdmin(request.user!.roles)
      const verified = app.licensing.verifyLicense(request.body.token)
      if (verified.status === 'invalid') throw new HttpProblem(400, 'invalid_license', `the license is not valid (${verified.error})`)
      if (verified.status === 'expired') throw new HttpProblem(400, 'invalid_license', 'the license has expired')
      const { claims } = verified
      const orgId = claims.orgId ?? null
      const userId = orgId ? null : claims.sub
      const db = database()
      await db.transaction(async (tx) => {
        if (orgId) {
          const [org] = await tx.select({ id: schema.organization.id }).from(schema.organization).where(eq(schema.organization.id, orgId))
          if (!org) throw new HttpProblem(400, 'invalid_license', 'the license is for an organization this server does not have')
        } else {
          await tx.insert(schema.kcUser).values({ id: claims.sub }).onConflictDoNothing()
        }
        const id = uuidFor(`license:${orgId ?? claims.sub}`)
        const values = {
          userId,
          orgId,
          plan: claims.plan,
          seats: claims.seats ?? null,
          features: claims.features,
          validUntil: claims.exp * 1000,
          token: request.body.token,
          updatedAt: Date.now(),
        }
        await tx.insert(schema.entitlement).values({ id, ...values }).onConflictDoUpdate({ target: schema.entitlement.id, set: values })
      })
      return {
        plan: verified.plan,
        features: verified.features,
        status: verified.status,
        orgId,
        userId,
        seats: verified.seats ?? null,
        expiresAt: claims.exp,
      }
    },
  )

  app.get(
    '/license',
    {
      schema: {
        tags: ['licensing'],
        summary: 'The licenses installed on this server',
        security: SECURITY,
        response: { 200: Type.Array(License), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      requireAdmin(request.user!.roles)
      const rows = await database().select().from(schema.entitlement).where(isNotNull(schema.entitlement.token))
      return rows.flatMap((row) => {
        const verified = app.licensing.verifyLicense(row.token!)
        if (verified.status === 'invalid') return []
        return [
          {
            plan: verified.plan,
            features: verified.features,
            status: verified.status,
            orgId: row.orgId,
            userId: row.userId,
            seats: verified.seats ?? null,
            expiresAt: verified.claims.exp,
          },
        ]
      })
    },
  )
}
