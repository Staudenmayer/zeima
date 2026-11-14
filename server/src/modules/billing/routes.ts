// Billing for the cloud edition (SPEC §2): checkout links and the Lemon
// Squeezy webhook that turns subscriptions into entitlements. Registered only
// when DEPLOYMENT_MODE=cloud; self-hosted installs use license files.
//
//   GET  /v1/billing/plans      what can be bought
//   POST /v1/billing/checkout   a purchase link for the caller
//   GET  /v1/billing            the caller's subscription, if any
//   POST /v1/billing/webhook    Lemon Squeezy → entitlements (signed, no bearer token)

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { desc, eq, or } from 'drizzle-orm'
import { Type } from 'typebox'
import type { Config } from '../../config.ts'
import { callerOf, schema, withUser } from '../../db/index.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { checkoutUrl, parseEvent, signatureValid } from './lemonsqueezy.ts'

const Plan = Type.Object({ key: Type.String(), plan: Type.String() })

const Subscription = Type.Object({
  plan: Type.String(),
  status: Type.String(),
  validUntil: Type.Union([Type.Integer(), Type.Null()], { description: 'Unix ms' }),
  portalUrl: Type.Union([Type.String(), Type.Null()], { description: 'Where the customer manages the subscription' }),
})

export const billingRoutes: FastifyPluginAsyncTypebox<{ config: Config }> = async (app, { config }) => {
  const { billing } = config
  const database = () => {
    if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
    return app.db
  }

  app.get(
    '/billing/plans',
    {
      schema: {
        tags: ['billing'],
        summary: 'What can be bought',
        security: SECURITY,
        response: { 200: Type.Array(Plan), ...problems(401, 403) },
      },
    },
    async () => billing.plans.map(({ key, plan }) => ({ key, plan })),
  )

  app.post(
    '/billing/checkout',
    {
      schema: {
        tags: ['billing'],
        summary: 'A purchase link for the caller',
        description: 'Open the link in a browser. The purchase turns into an entitlement when the payment provider confirms it.',
        security: SECURITY,
        body: Type.Object({ key: Type.String(), orgId: Type.Optional(Type.String({ format: 'uuid' })) }),
        response: { 200: Type.Object({ url: Type.String() }), ...problems(401, 403, 404, 503) },
      },
    },
    async (request) => {
      if (!billing.storeUrl) throw new HttpProblem(503, 'billing_unavailable', 'purchases are not set up on this server')
      const plan = billing.plans.find((p) => p.key === request.body.key)
      if (!plan) throw new HttpProblem(404, 'not_found', 'no such plan')
      const { orgId } = request.body
      const user = request.user!
      // Only an organization's owner buys for it.
      if (orgId && !user.orgs[orgId]?.permissions['billing.manage']) throw new HttpProblem(403, 'forbidden', 'only an organization owner can buy for it')
      return { url: checkoutUrl(billing.storeUrl, plan, { userId: user.id, orgId, email: user.email }) }
    },
  )

  app.get(
    '/billing',
    {
      schema: {
        tags: ['billing'],
        summary: 'The caller\'s subscription',
        security: SECURITY,
        response: { 200: Type.Union([Subscription, Type.Null()]), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const user = request.user!
      const orgIds = Object.keys(user.orgs)
      const rows = await withUser(database(), callerOf(user), (tx) =>
        tx
          .select()
          .from(schema.entitlement)
          .where(
            or(
              eq(schema.entitlement.userId, user.id),
              ...orgIds.map((id) => eq(schema.entitlement.orgId, id)),
            ),
          )
          .orderBy(desc(schema.entitlement.updatedAt)),
      )
      const row = rows.find((r) => r.billing)
      if (!row) return null
      const b = row.billing as { status?: string; portalUrl?: string | null }
      return { plan: row.plan, status: b.status ?? 'unknown', validUntil: row.validUntil, portalUrl: b.portalUrl ?? null }
    },
  )

  // The webhook is signed over the exact bytes, so it gets the raw body.
  await app.register(async (hooks) => {
    hooks.addContentTypeParser('application/json', { parseAs: 'buffer' }, (_request, body, done) => done(null, body))

    hooks.post(
      '/billing/webhook',
      {
        schema: {
          tags: ['billing'],
          summary: 'Lemon Squeezy webhook',
          description: 'Signed with the webhook secret (`X-Signature`, HMAC-SHA256 of the body). Not for clients.',
          body: Type.Unsafe<Buffer>({}),
          response: {
            200: Type.Object({ status: Type.Union([Type.Literal('applied'), Type.Literal('ignored')]), reason: Type.Optional(Type.String()) }),
            ...problems(401, 503),
          },
        },
      },
      async (request) => {
        if (!billing.webhookSecret) throw new HttpProblem(503, 'billing_unavailable', 'the billing webhook is not set up')
        const raw = request.body as Buffer
        const signature = request.headers['x-signature']
        if (!signatureValid(raw, typeof signature === 'string' ? signature : undefined, billing.webhookSecret)) {
          throw new HttpProblem(401, 'unauthorized', 'the signature is not valid')
        }
        let body: unknown
        try {
          body = JSON.parse(raw.toString('utf8'))
        } catch {
          return { status: 'ignored' as const, reason: 'not JSON' }
        }
        const parsed = parseEvent(body, billing.plans, Date.now())
        if (!parsed.ok) {
          request.log.warn({ reason: parsed.reason }, 'billing event ignored')
          return { status: 'ignored' as const, reason: parsed.reason }
        }
        const { update } = parsed
        const db = database()

        // Written as the table owner, like the rest of the identity data.
        return db.transaction(async (tx) => {
          if (update.orgId) {
            const [org] = await tx.select({ id: schema.organization.id }).from(schema.organization).where(eq(schema.organization.id, update.orgId))
            if (!org) return { status: 'ignored' as const, reason: 'unknown organization' }
          } else if (update.userId) {
            await tx.insert(schema.kcUser).values({ id: update.userId, email: update.email }).onConflictDoNothing()
          }
          const [existing] = await tx.select({ updatedAt: schema.entitlement.updatedAt }).from(schema.entitlement).where(eq(schema.entitlement.id, update.id))
          if (existing && existing.updatedAt > update.updatedAt) return { status: 'ignored' as const, reason: 'older than what is stored' }
          const values = {
            userId: update.userId,
            orgId: update.orgId,
            plan: update.plan,
            validUntil: update.validUntil,
            billing: update.billing,
            updatedAt: update.updatedAt,
          }
          await tx
            .insert(schema.entitlement)
            .values({ id: update.id, ...values })
            .onConflictDoUpdate({ target: schema.entitlement.id, set: values })
          return { status: 'applied' as const }
        })
      },
    )
  })
}
