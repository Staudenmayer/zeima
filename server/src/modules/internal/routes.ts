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

export function sign(secret: string, body: Buffer | string): string {
  return `sha256=${createHmac('sha256', secret).update(body).digest('hex')}`
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
      schema: {
        tags: ['internal'],
        summary: 'Keycloak says something changed (signed)',
        description:
          'Sent by the Keycloak event listener for membership, group and user events. The body is signed with HMAC-SHA256 using KEYCLOAK_WEBHOOK_SECRET in the header `x-zeima-signature` (`sha256=<hex>`). ' +
          'Give `orgId` to check one organization, or `userId` to check the organizations that person is in.',
        headers: Type.Object({ 'x-zeima-signature': Type.String() }),
        body: Type.Object({
          type: Type.String({ maxLength: 100 }),
          orgId: Type.Optional(Type.String({ format: 'uuid' })),
          userId: Type.Optional(Type.String({ format: 'uuid' })),
        }),
        response: { 202: Type.Object({ organizations: Type.Integer() }), ...problems(400, 401, 503) },
      },
    },
    async (request, reply) => {
      const given = Buffer.from(request.headers['x-zeima-signature'])
      const expected = Buffer.from(sign(secret, request.rawBody ?? ''))
      if (given.length !== expected.length || !timingSafeEqual(given, expected)) {
        throw new HttpProblem(401, 'unauthorized', 'the signature does not match')
      }
      if (!app.db || !app.keycloakAdmin) throw new HttpProblem(503, 'unavailable', 'organization management is not set up')
      const { orgId, userId } = request.body
      const orgs = orgId ? [orgId] : userId ? await orgsOfUser(app.db, userId) : []
      // Answer now; the check takes a few calls to Keycloak.
      for (const id of orgs) {
        void reconcileOrganization(app.db, app.keycloakAdmin, id).catch((err: unknown) => app.log.warn({ err, orgId: id }, 'reconcile after event failed'))
      }
      return reply.code(202).send({ organizations: orgs.length })
    },
  )
}
