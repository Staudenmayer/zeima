// SCIM 2.0 provisioning (RFC 7643/7644), Phase 7: an organization's identity provider (Entra ID, Okta, …)
// creates, changes, disables and removes the organization's people by itself.
//
// Two parts:
//   /v1/orgs/:orgId/scim…   an owner or admin makes the keys (Keycloak sign-in, `org.settings`)
//   /v1/scim/v2/…           what the identity provider calls; a key identifies the organization
//
// Only people whose address is in a domain the organization has set up for single sign-on can be
// provisioned: nobody can create or disable accounts outside their own domains. Owners are never
// changed by SCIM. Users only; groups are not offered.

import { createHash, randomBytes } from 'node:crypto'
import type { FastifyPluginAsyncTypebox, TypeBoxTypeProvider } from '@fastify/type-provider-typebox'
import { and, desc, eq, inArray, sql } from 'drizzle-orm'
import type { FastifyReply, FastifyRequest } from 'fastify'
import { Type } from 'typebox'
import type { Config } from '../../config.ts'
import { type Db, schema } from '../../db/index.ts'
import { lockPushOrder } from '../../db/locks.ts'
import type { KeycloakAdmin } from '../../keycloak/admin.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { authorize } from '../../plugins/authz.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { forgetMembership } from './roles.ts'
import { audit, listMembers, type MemberRow, purgeMemberOrgData, requireSeat, seatUsage, withOwnerLock } from './service.ts'

const OrgParams = Type.Object({ orgId: Type.String({ format: 'uuid' }) })
const USER = 'urn:ietf:params:scim:schemas:core:2.0:User'
const ERROR = 'urn:ietf:params:scim:api:messages:2.0:Error'
const LIST = 'urn:ietf:params:scim:api:messages:2.0:ListResponse'

export const hashToken = (token: string) => createHash('sha256').update(token).digest('hex')

/** An error in SCIM's own shape. */
export class ScimError extends Error {
  readonly status: number
  readonly scimType?: string
  constructor(status: number, detail: string, scimType?: string) {
    super(detail)
    this.status = status
    this.scimType = scimType
  }
}

declare module 'fastify' {
  interface FastifyRequest {
    /** The organization a SCIM key belongs to. */
    scimOrg?: string
    /** The id of the SCIM key that made the call (for the audit log). */
    scimKey?: string
  }
}

const Tokens = Type.Object({
  baseUrl: Type.String({ description: 'Give this to the identity provider as the SCIM address (tenant URL)' }),
  tokens: Type.Array(
    Type.Object({
      id: Type.String(),
      name: Type.String(),
      hint: Type.String({ description: 'The first characters of the key' }),
      createdAt: Type.Integer(),
      lastUsedAt: Type.Union([Type.Integer(), Type.Null()]),
    }),
  ),
})

const Any = Type.Record(Type.String(), Type.Any())
/** The SCIM routes check their own key (`onRequest` below), not a Keycloak token. */
/** Own, lower limit than the rest of the API: provisioning is bursty but not a thousand calls a minute. */
const SCIM_PUBLIC = { public: 'a SCIM key', rateLimit: { max: 600, timeWindow: '1 minute' } }

/** What the documentation needs of a route the identity provider calls (the bodies are SCIM's own). */
const doc = (summary: string, ok: number = 200) => ({
  tags: ['scim'],
  summary,
  description: 'SCIM 2.0. Sign in with a key made under the organization\'s single sign-on settings (`Authorization: Bearer zscim_…`).',
  response: { [ok]: ok === 204 ? Type.Null() : Any },
})
const IdParams = Type.Object({ id: Type.String({ format: 'uuid' }) })

/** A member as a SCIM user. */
export function toScim(m: MemberRow, baseUrl: string) {
  const [given, ...rest] = (m.name ?? '').split(' ')
  return {
    schemas: [USER],
    id: m.id,
    userName: m.email ?? m.id,
    name: { formatted: m.name ?? '', givenName: given ?? '', familyName: rest.join(' ') },
    displayName: m.name ?? m.email ?? '',
    emails: m.email ? [{ value: m.email, primary: true, type: 'work' }] : [],
    active: m.active,
    meta: { resourceType: 'User', location: `${baseUrl}/Users/${m.id}` },
  }
}

/** `userName eq "x"` (also `emails.value`); anything else is refused. */
export function emailFromFilter(filter: string | undefined): string | undefined {
  if (!filter) return undefined
  const m = /^\s*(?:userName|emails\.value)\s+eq\s+"([^"]{1,254})"\s*$/i.exec(filter)
  if (!m) throw new ScimError(400, 'only the filter userName eq "…" is supported', 'invalidFilter')
  return m[1]!.toLowerCase()
}

const truthy = (v: unknown) => (typeof v === 'string' ? v.toLowerCase() === 'true' : v === true)

export interface Changes {
  active?: boolean
  givenName?: string
  familyName?: string
}

/** What a PATCH (or PUT) body changes. */
export function changesOf(body: Record<string, unknown>, patch: boolean): Changes {
  const out: Changes = {}
  const set = (path: string, value: unknown) => {
    const p = path.toLowerCase()
    if (p === 'active') out.active = truthy(value)
    else if (p === 'name.givenname') out.givenName = String(value ?? '')
    else if (p === 'name.familyname') out.familyName = String(value ?? '')
    else if (p === 'name' && value && typeof value === 'object') {
      const v = value as Record<string, unknown>
      if (v.givenName !== undefined) out.givenName = String(v.givenName)
      if (v.familyName !== undefined) out.familyName = String(v.familyName)
    }
  }
  if (!patch) {
    if (body.active !== undefined) out.active = truthy(body.active)
    if (body.name) set('name', body.name)
    return out
  }
  const ops = body.Operations ?? body.operations
  if (!Array.isArray(ops)) throw new ScimError(400, 'Operations is required', 'invalidSyntax')
  for (const op of ops as Record<string, unknown>[]) {
    const kind = String(op.op ?? '').toLowerCase()
    if (kind !== 'replace' && kind !== 'add') continue // nothing here can be removed
    if (typeof op.path === 'string') set(op.path, op.value)
    else if (op.value && typeof op.value === 'object') {
      for (const [k, v] of Object.entries(op.value as Record<string, unknown>)) set(k, v)
    }
  }
  return out
}

export const scimRoutes: FastifyPluginAsyncTypebox<{ config: Config }> = async (app, { config }) => {
  const database = (): Db => {
    if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not set up')
    return app.db
  }
  const keycloak = (): KeycloakAdmin => {
    if (!app.keycloakAdmin) throw new HttpProblem(503, 'unavailable', 'organization management is not set up')
    return app.keycloakAdmin
  }
  const baseUrl = `${config.publicUrl}/v1/scim/v2`

  // ── Keys, for owners and admins ────────────────────────────────────────────

  const view = async (orgId: string) => {
    const rows = await database().select().from(schema.scimToken).where(eq(schema.scimToken.orgId, orgId)).orderBy(schema.scimToken.createdAt)
    return { baseUrl, tokens: rows.map((r) => ({ id: r.id, name: r.name, hint: r.hint, createdAt: r.createdAt, lastUsedAt: r.lastUsedAt })) }
  }

  const guard = async (request: FastifyRequest, orgId: string) => {
    const user = request.user!
    if (!user.orgs[orgId]) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
    authorize({ user, orgId }, 'org.settings', { orgId })
    if (app.licensing.enforced) await app.licensing.require(database(), user, 'sso')
    return user
  }

  app.get(
    '/orgs/:orgId/scim',
    {
      schema: {
        tags: ['sso'],
        summary: 'The address and the keys for SCIM provisioning',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Tokens, ...problems(401, 403) },
      },
    },
    async (request) => {
      await guard(request, request.params.orgId)
      return view(request.params.orgId)
    },
  )

  app.post(
    '/orgs/:orgId/scim/tokens',
    {
      schema: {
        tags: ['sso'],
        summary: 'Makes a key for the identity provider',
        description: 'The key is in the answer once and cannot be read again.',
        security: SECURITY,
        params: OrgParams,
        body: Type.Object({ name: Type.String({ minLength: 1, maxLength: 80 }) }),
        response: { 201: Type.Object({ id: Type.String(), token: Type.String() }), ...problems(400, 401, 403, 409) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const user = await guard(request, orgId)
      const db = database()
      if ((await view(orgId)).tokens.length >= 5) throw new HttpProblem(409, 'conflict', 'an organization can have five keys; remove one first')
      const token = `zscim_${randomBytes(32).toString('base64url')}`
      const [row] = await db
        .insert(schema.scimToken)
        .values({ orgId, name: request.body.name, hash: hashToken(token), hint: token.slice(0, 10), createdBy: user.id, createdAt: Date.now() })
        .returning({ id: schema.scimToken.id })
      await audit(db, { orgId, actorId: user.id, action: 'scim.token.create', target: row!.id, detail: { name: request.body.name } })
      return reply.code(201).send({ id: row!.id, token })
    },
  )

  app.delete(
    '/orgs/:orgId/scim/tokens/:id',
    {
      schema: {
        tags: ['sso'],
        summary: 'Removes a key; the identity provider stops being able to provision',
        security: SECURITY,
        params: Type.Object({ orgId: Type.String({ format: 'uuid' }), id: Type.String({ format: 'uuid' }) }),
        response: { 204: Type.Null(), ...problems(401, 403, 404) },
      },
    },
    async (request, reply) => {
      const { orgId, id } = request.params
      const user = await guard(request, orgId)
      const db = database()
      const gone = await db.delete(schema.scimToken).where(and(eq(schema.scimToken.id, id), eq(schema.scimToken.orgId, orgId))).returning({ id: schema.scimToken.id })
      if (gone.length === 0) throw new HttpProblem(404, 'not_found', 'there is no such key')
      await audit(db, { orgId, actorId: user.id, action: 'scim.token.delete', target: id })
      return reply.code(204).send(null)
    },
  )

  // ── SCIM, for the identity provider ────────────────────────────────────────

  await app.register(
    async (plain) => {
      const scim = plain.withTypeProvider<TypeBoxTypeProvider>()
      scim.addContentTypeParser(['application/scim+json', 'application/json'], { parseAs: 'string' }, (_request, body, done) => {
        try {
          done(null, body === '' ? {} : JSON.parse(body as string))
        } catch {
          done(new ScimError(400, 'the body is not valid JSON', 'invalidSyntax'))
        }
      })

      scim.setErrorHandler((error: Error & { statusCode?: number; validation?: unknown }, request, reply) => {
        let status = 500
        let detail = 'something went wrong'
        let scimType: string | undefined
        if (error instanceof ScimError) {
          status = error.status
          detail = error.message
          scimType = error.scimType
        } else if (error instanceof HttpProblem) {
          status = error.status
          detail = error.message
          if (status === 409 && error.code !== 'seat_limit_reached') scimType = 'uniqueness'
        } else if (error.validation) {
          status = 400
          detail = error.message
          scimType = 'invalidValue'
        } else if (error.statusCode && error.statusCode >= 400 && error.statusCode < 500) {
          status = error.statusCode
          detail = error.message
        } else {
          request.log.error({ err: error }, 'scim request failed')
        }
        return reply
          .code(status)
          .type('application/scim+json')
          .send({ schemas: [ERROR], status: String(status), detail, ...(scimType && { scimType }) })
      })

      scim.setNotFoundHandler((_request, reply) => reply.code(404).type('application/scim+json').send({ schemas: [ERROR], status: '404', detail: 'not found' }))

      // The key says which organization this is.
      scim.addHook('onRequest', async (request) => {
        const token = request.headers.authorization?.match(/^Bearer\s+(\S+)$/i)?.[1]
        if (!token) throw new ScimError(401, 'a bearer key is required')
        const db = database()
        const [row] = await db.select().from(schema.scimToken).where(eq(schema.scimToken.hash, hashToken(token)))
        if (!row) throw new ScimError(401, 'the key is not valid')
        request.scimOrg = row.orgId
        request.scimKey = row.id
        if (!row.lastUsedAt || Date.now() - row.lastUsedAt > 60_000) {
          await db.update(schema.scimToken).set({ lastUsedAt: Date.now() }).where(eq(schema.scimToken.id, row.id))
        }
        await app.licensing.requireForOrg(db, row.orgId, 'sso')
      })

      const orgOf = (request: FastifyRequest) => request.scimOrg!
      const send = (reply: FastifyReply, code: number, body: unknown) => reply.code(code).type('application/scim+json').send(body)

      /** The domains the organization has set up for sign-in: only their people can be provisioned. */
      const claimedDomains = async (orgId: string) => (await keycloak().getSso(orgId))?.domains ?? []
      const inClaimedDomain = (email: string, domains: string[]) => domains.includes(email.split('@')[1] ?? '')

      const member = async (orgId: string, id: string): Promise<MemberRow> => {
        const found = (await listMembers(database(), orgId, { userIds: [id] }))[0]
        if (!found) throw new ScimError(404, 'there is no such user')
        return found
      }

      /** A member SCIM may change: not an owner, and in a domain of the organization. */
      const changeable = async (orgId: string, m: MemberRow) => {
        // Provisioning is not bound by the role hierarchy (a key has no person behind it), so it gets a fixed ceiling:
        // it never touches anyone who administers the organization (admins, owners, custom roles).
        if (m.role !== 'member' && m.role !== 'manager') throw new ScimError(403, 'only members and managers are changed by provisioning')
        if (!m.email || !inClaimedDomain(m.email, await claimedDomains(orgId))) {
          throw new ScimError(403, 'this person is not in a domain of the organization', 'mutability')
        }
      }

      /** Whether the person is a member of another organization too. */
      const belongsElsewhere = async (orgId: string, userId: string) => {
        const rows = await database().select({ orgId: schema.kcOrgMember.orgId }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.userId, userId))
        return rows.some((r) => r.orgId !== orgId)
      }

      /**
       * Takes the person out of Keycloak and the mirror under the organization lock (then the push lock), like the route
       * that removes a member, so a check against Keycloak that read the list before the removal cannot put them back.
       */
      const removeFromOrganization = (orgId: string, userId: string) =>
        withOwnerLock(database(), orgId, async (tx) => {
          await keycloak().removeMember(orgId, userId)
          await lockPushOrder(tx)
          await tx.delete(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.userId, userId)))
          await tx.execute(sql`delete from kc_group_member where user_id = ${userId} and group_id in (select id from kc_group where org_id = ${orgId})`)
          await purgeMemberOrgData(tx, orgId, userId)
        })

      const apply = async (orgId: string, m: MemberRow, changes: Changes, key: string | undefined): Promise<'removed' | 'changed'> => {
        const db = database()
        const kc = keycloak()
        if (changes.givenName !== undefined || changes.familyName !== undefined) {
          const [given = '', ...rest] = (m.name ?? '').split(' ')
          const first = changes.givenName ?? given
          const last = changes.familyName ?? rest.join(' ')
          await kc.setUserName(m.id, first, last)
          await db.update(schema.kcUser).set({ displayName: `${first} ${last}`.trim() || null }).where(eq(schema.kcUser.id, m.id))
        }
        if (changes.active === false && m.active && (await belongsElsewhere(orgId, m.id))) {
          // The account is also somebody else's (another organization, or the person's own); switching it off
          // would lock them out of those too. Taking them out of this organization ends this organization's access.
          await removeFromOrganization(orgId, m.id)
          forgetMembership(m.id)
          await audit(db, { orgId, actorId: null, action: 'member.remove', target: m.id, detail: { via: 'scim', key, reason: 'deactivated, account belongs to other organizations' } })
          return 'removed' as const
        }
        if (changes.active === true && !m.active) {
          // Provisioning switches back on only what provisioning switched off: an administrator's (or the operator's)
          // deactivation is theirs to undo.
          const [last] = await db
            .select({ action: schema.auditLog.action, detail: schema.auditLog.detail })
            .from(schema.auditLog)
            .where(and(eq(schema.auditLog.orgId, orgId), eq(schema.auditLog.target, m.id), inArray(schema.auditLog.action, ['member.deactivate', 'member.reactivate'])))
            .orderBy(desc(schema.auditLog.id))
            .limit(1)
          if (last?.action !== 'member.deactivate' || (last.detail as { via?: string }).via !== 'scim') {
            throw new ScimError(403, 'this account was switched off by an administrator; only an administrator switches it back on', 'mutability')
          }
        }
        if (changes.active !== undefined && changes.active !== m.active) {
          await kc.setEnabled(m.id, changes.active)
          if (!changes.active) await kc.logoutUser(m.id).catch(() => {})
          await db.update(schema.kcUser).set({ active: changes.active }).where(eq(schema.kcUser.id, m.id))
          await audit(db, { orgId, actorId: null, action: changes.active ? 'member.reactivate' : 'member.deactivate', target: m.id, detail: { via: 'scim', key } })
        }
        return 'changed'
      }

      scim.get('/ServiceProviderConfig', { config: SCIM_PUBLIC, schema: doc('What this SCIM service can do') }, async (_request, reply) =>
        send(reply, 200, {
          schemas: ['urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig'],
          patch: { supported: true },
          bulk: { supported: false, maxOperations: 0, maxPayloadSize: 0 },
          filter: { supported: true, maxResults: 200 },
          changePassword: { supported: false },
          sort: { supported: false },
          etag: { supported: false },
          authenticationSchemes: [{ type: 'oauthbearertoken', name: 'Bearer key', description: 'A key made in Zeima, sent in the Authorization header' }],
        }),
      )

      scim.get('/ResourceTypes', { config: SCIM_PUBLIC, schema: doc('The kinds of resources') }, async (_request, reply) =>
        send(reply, 200, {
          schemas: [LIST],
          totalResults: 1,
          Resources: [{ schemas: ['urn:ietf:params:scim:schemas:core:2.0:ResourceType'], id: 'User', name: 'User', endpoint: '/Users', schema: USER }],
        }),
      )

      scim.get('/Schemas', { config: SCIM_PUBLIC, schema: doc('The schemas') }, async (_request, reply) => send(reply, 200, { schemas: [LIST], totalResults: 0, Resources: [] }))

      scim.get('/Users', { config: SCIM_PUBLIC, schema: {
            ...doc('Lists the organization\'s people, or finds one by `userName eq "…"`'),
            querystring: Type.Object({
              filter: Type.Optional(Type.String()),
              startIndex: Type.Optional(Type.Integer({ minimum: 1 })),
              count: Type.Optional(Type.Integer({ minimum: 0, maximum: 200 })),
            }),
          },
        },
        async (request, reply) => {
          const email = emailFromFilter(request.query.filter)
          const all = (await listMembers(database(), orgOf(request))).filter((m) => email === undefined || m.email?.toLowerCase() === email)
          const start = request.query.startIndex ?? 1
          const page = all.slice(start - 1, start - 1 + (request.query.count ?? 100))
          return send(reply, 200, { schemas: [LIST], totalResults: all.length, startIndex: start, itemsPerPage: page.length, Resources: page.map((m) => toScim(m, baseUrl)) })
        },
      )

      scim.get('/Users/:id', { config: SCIM_PUBLIC, schema: { ...doc('One person'), params: IdParams } }, async (request, reply) => send(reply, 200, toScim(await member(orgOf(request), request.params.id), baseUrl)))

      scim.post('/Users', { config: SCIM_PUBLIC, schema: { ...doc('Adds a person to the organization', 201), body: Any } }, async (request, reply) => {
        const orgId = orgOf(request)
        const body = request.body as Record<string, unknown>
        const emails = Array.isArray(body.emails) ? (body.emails as { value?: unknown; primary?: unknown }[]) : []
        const raw = (typeof body.userName === 'string' && body.userName.includes('@') ? body.userName : null) ?? emails.find((e) => e.primary === true)?.value ?? emails[0]?.value
        const email = typeof raw === 'string' ? raw.trim().toLowerCase() : ''
        if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) throw new ScimError(400, 'userName must be an email address', 'invalidValue')
        if (!inClaimedDomain(email, await claimedDomains(orgId))) {
          throw new ScimError(400, 'the address is not in a domain the organization has set up for single sign-on', 'invalidValue')
        }
        const name = (body.name ?? {}) as { givenName?: unknown; familyName?: unknown; formatted?: unknown }
        const [formattedFirst = '', ...formattedRest] = typeof name.formatted === 'string' ? name.formatted.split(' ') : []
        const firstName = typeof name.givenName === 'string' ? name.givenName : formattedFirst
        const lastName = typeof name.familyName === 'string' ? name.familyName : formattedRest.join(' ')
        const active = body.active === undefined ? true : truthy(body.active)

        const db = database()
        const kc = keycloak()
        const existing = await kc.findUserByEmail(email)
        if (existing && (await listMembers(db, orgId, { userIds: [existing.id] })).length) throw new ScimError(409, 'a user with this userName already exists', 'uniqueness')
        requireSeat((await app.licensing.ofOrg(db, orgId))?.seats ?? null, await seatUsage(db, orgId))

        if (existing) {
          // The address already has an account that this organization did not create: it does not get pulled in
          // without the person's say. They are invited, and join when they accept.
          const [row] = await db.insert(schema.orgInvitation).values({ orgId, email, role: 'member', createdAt: Date.now() }).onConflictDoNothing().returning({ id: schema.orgInvitation.id })
          if (row) {
            await kc.invite(orgId, email).catch(async (err: unknown) => {
              await db.delete(schema.orgInvitation).where(eq(schema.orgInvitation.id, row.id))
              throw err
            })
            await audit(db, { orgId, actorId: null, action: 'member.invite', target: email, detail: { role: 'member', via: 'scim', key: request.scimKey } })
          }
          // The same answer as for a person who is already in: what the key holder learns is that this address is taken, not
          // whether it belongs to somebody else's account.
          throw new ScimError(409, 'a user with this userName already exists', 'uniqueness')
        }
        const userId = await kc.createUser({ email, firstName, lastName, enabled: active, emailVerified: config.sso.verifyDomains })
        await kc.addMember(orgId, userId)
        await kc.setRole(orgId, userId, 'member')
        const displayName = `${firstName} ${lastName}`.trim() || null
        const enabled = active
        await db.transaction(async (tx) => {
          await tx
            .insert(schema.kcUser)
            .values({ id: userId, email, displayName, active: enabled })
            .onConflictDoUpdate({ target: schema.kcUser.id, set: { email, active: enabled, ...(displayName ? { displayName } : {}) } })
          await tx.insert(schema.kcOrgMember).values({ orgId, userId, role: 'member' }).onConflictDoNothing()
        })
        forgetMembership(userId)
        await audit(db, { orgId, actorId: null, action: 'member.join', target: userId, detail: { role: 'member', via: 'scim', key: request.scimKey } })
        return reply.header('location', `${baseUrl}/Users/${userId}`).code(201).type('application/scim+json').send(toScim(await member(orgId, userId), baseUrl))
      })

      const change =
        (patch: boolean) =>
        async (request: FastifyRequest<{ Params: { id: string }; Body: Record<string, unknown> }>, reply: FastifyReply) => {
          const orgId = orgOf(request)
          const m = await member(orgId, request.params.id)
          await changeable(orgId, m)
          const outcome = await apply(orgId, m, changesOf(request.body, patch), request.scimKey)
          // A person who was only taken out of this organization is reported as switched off, which is what the provider asked for.
          return send(reply, 200, outcome === 'removed' ? toScim({ ...m, active: false }, baseUrl) : toScim(await member(orgId, m.id), baseUrl))
        }
      scim.put('/Users/:id', { config: SCIM_PUBLIC, schema: { ...doc('Replaces a person\'s name and whether the account is active'), params: IdParams, body: Any } }, change(false))
      scim.patch('/Users/:id', { config: SCIM_PUBLIC, schema: { ...doc('Changes whether the account is active, or the name'), params: IdParams, body: Any } }, change(true))

      scim.delete('/Users/:id', { config: SCIM_PUBLIC, schema: { ...doc('Takes a person out of the organization', 204), params: IdParams } }, async (request, reply) => {
        const orgId = orgOf(request)
        const m = await member(orgId, request.params.id)
        await changeable(orgId, m)
        const db = database()
        await removeFromOrganization(orgId, m.id)
        forgetMembership(m.id)
        await audit(db, { orgId, actorId: null, action: 'member.remove', target: m.id, detail: { via: 'scim', key: request.scimKey } })
        return reply.code(204).send(null)
      })
    },
    { prefix: '/scim/v2' },
  )
}
