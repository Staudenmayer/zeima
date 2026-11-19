// Calls from Keycloak's event listener (SPEC §7.2, mirror path 2). The
// listener signs the body with HMAC-SHA256 (`x-zeima-signature: sha256=<hex>`)
// and says what changed; the API then re-reads that organization from
// Keycloak. The event carries no trust: it only says where to look.

import { createHmac, timingSafeEqual } from 'node:crypto'
import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'
import type { Config } from '../../config.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { orgsOfUser, reconcileOrganization } from '../orgs/reconcile.ts'

declare module 'fastify' {
  interface FastifyRequest {
    rawBody?: Buffer
  }
}

/** The call is refused when its timestamp is further than this from the server's clock, so a captured call cannot be replayed later. */
export const MAX_WEBHOOK_AGE_MS = 5 * 60_000

/** `sha256=<hex>`: HMAC-SHA256 over `<timestamp>.<body>`. */
export function sign(secret: string, body: Buffer | string, timestamp: number | string): string {
  return `sha256=${createHmac('sha256', secret).update(`${timestamp}.`).update(body).digest('hex')}`
}

export const internalRoutes: FastifyPluginAsyncTypebox<{ config: Config }> = async (app, { config }) => {
  const secret = config.keycloakWebhookSecret
  if (!secret) return

  // The signature is over the bytes that were sent: keep them.
  app.addContentTypeParser('application/json', { parseAs: 'buffer' }, (request, body, done) => {
    request.rawBody = body as Buffer
    try {
      done(null, JSON.parse((body as Buffer).toString('utf8')))
    } catch (err) {
      done(err as Error)
    }
  })

  app.post(
    '/internal/keycloak-events',
    {
      config: { public: 'signed by Keycloak (HMAC)' },
      schema: {
        tags: ['internal'],
        summary: 'Keycloak says something changed (signed)',
        description:
          'Sent by the Keycloak event listener for membership, group and user events. The signature is HMAC-SHA256 over `<timestamp>.<body>` using KEYCLOAK_WEBHOOK_SECRET, in the header `x-zeima-signature` (`sha256=<hex>`), with the time in milliseconds in `x-zeima-timestamp`; a call more than five minutes off the server\'s clock is refused. ' +
          'Give `orgId` to check one organization, or `userId` to check the organizations that person is in.',
        headers: Type.Object({ 'x-zeima-signature': Type.String(), 'x-zeima-timestamp': Type.String({ pattern: '^[0-9]{10,16}$' }) }),
        body: Type.Object({
          type: Type.String({ maxLength: 100 }),
          orgId: Type.Optional(Type.String({ format: 'uuid' })),
          userId: Type.Optional(Type.String({ format: 'uuid' })),
        }),
        response: { 202: Type.Object({ organizations: Type.Integer() }), ...problems(400, 401, 503) },
      },
    },
    async (request, reply) => {
      const stamp = request.headers['x-zeima-timestamp']
      const given = Buffer.from(request.headers['x-zeima-signature'])
      const expected = Buffer.from(sign(secret, request.rawBody ?? '', stamp))
      if (given.length !== expected.length || !timingSafeEqual(given, expected)) {
        throw new HttpProblem(401, 'unauthorized', 'the signature does not match')
      }
      // Signed correctly, but is it fresh? Checked after the signature so the clock is not probed by strangers.
      if (Math.abs(Date.now() - Number(stamp)) > MAX_WEBHOOK_AGE_MS) throw new HttpProblem(401, 'unauthorized', 'the call is too old (or the clocks differ by more than five minutes)')
      if (!app.db || !app.keycloakAdmin) throw new HttpProblem(503, 'unavailable', 'organization management is not set up')
      const { orgId, userId } = request.body
      const orgs = orgId ? [orgId] : userId ? await orgsOfUser(app.db, userId) : []
      // Answer now; the check takes a few calls to Keycloak.
      const { db, keycloakAdmin: kc } = app
      const seatLimit = async (orgId: string) => (await app.licensing.ofOrg(db, orgId))?.seats ?? null
      for (const id of orgs) {
        void reconcileOrganization(db, kc, id, { seatLimit }).catch((err: unknown) => app.log.warn({ err, orgId: id }, 'reconcile after event failed'))
      }
      return reply.code(202).send({ organizations: orgs.length })
    },
  )
}
