// Keeps the `kc_user` mirror current: every authenticated request upserts the
// caller from their token (SPEC §7.2, "token upsert"). Rows reference
// `kc_user`, so a user must exist before they can own data. Throttled per
// user; runs as the table owner, since requests may not write identity data.

import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { schema } from '../db/index.ts'

const REFRESH_MS = 5 * 60_000

export const identityPlugin = fp(async (app: FastifyInstance) => {
  const seen = new Map<string, number>()

  app.addHook('preHandler', async (request) => {
    const user = request.user
    if (!user || !app.db) return
    const now = Date.now()
    if (now - (seen.get(user.id) ?? 0) < REFRESH_MS) return
    await app.db
      .insert(schema.kcUser)
      .values({ id: user.id, email: user.email, displayName: user.name, locale: user.locale, lastSeenAt: now })
      .onConflictDoUpdate({
        target: schema.kcUser.id,
        set: { email: user.email, displayName: user.name, locale: user.locale, lastSeenAt: now },
      })
    seen.set(user.id, now)
  })
})
