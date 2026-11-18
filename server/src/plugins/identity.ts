// Keeps the `kc_user` mirror current: every authenticated request upserts the
// caller from their token (SPEC §7.2, "token upsert"). Rows reference
// `kc_user`, so a user must exist before they can own data. Throttled per
// user; runs as the table owner, since requests may not write identity data.

import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { eq } from 'drizzle-orm'
import { schema } from '../db/index.ts'
import { HttpProblem } from './errors.ts'
import { acceptInvitations } from '../modules/orgs/service.ts'

const REFRESH_MS = 5 * 60_000
/** How long an account's "disabled" state is remembered: a disabled account is shut out within this time. */
const ACTIVE_MS = 15_000

export const identityPlugin = fp(async (app: FastifyInstance) => {
  const seen = new Map<string, number>()
  const checked = new Map<string, { active: boolean; at: number }>()

  app.addHook('preHandler', async (request) => {
    const user = request.user
    if (!user || !app.db) return
    const now = Date.now()
    // A disabled account (an administrator, SCIM or the identity provider turned it off) works until its
    // token runs out unless it is refused here.
    const known = checked.get(user.id)
    let active = known && now - known.at < ACTIVE_MS ? known.active : undefined
    if (active === undefined) {
      const [row] = await app.db.select({ active: schema.kcUser.active }).from(schema.kcUser).where(eq(schema.kcUser.id, user.id))
      active = row?.active ?? true
      checked.set(user.id, { active, at: now })
      if (checked.size > 5_000) checked.clear()
    }
    if (!active) throw new HttpProblem(401, 'unauthorized', 'this account is disabled', { 'www-authenticate': 'Bearer realm="zeima", error="invalid_token", error_description="account disabled"' })
    if (now - (seen.get(user.id) ?? 0) < REFRESH_MS) return
    await app.db
      .insert(schema.kcUser)
      .values({ id: user.id, email: user.email, displayName: user.name, locale: user.locale, lastSeenAt: now })
      .onConflictDoUpdate({
        target: schema.kcUser.id,
        set: { email: user.email, displayName: user.name, locale: user.locale, lastSeenAt: now },
      })
    seen.set(user.id, now)
    // Invitations the person has accepted give them their role.
    if (app.keycloakAdmin) {
      await acceptInvitations(app.db, app.keycloakAdmin, user).catch((err: unknown) => app.log.warn({ err }, 'could not apply an invitation'))
    }
  })
})
