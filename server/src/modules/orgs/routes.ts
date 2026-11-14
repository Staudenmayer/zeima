// Organizations and their members (SPEC §5.9). Only people with the right
// role in the organization may call these (permissions.yaml); changes go to
// Keycloak first (src/keycloak/admin.ts), then to the mirror tables.

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { and, asc, eq, ne, sql } from 'drizzle-orm'
import { Type } from 'typebox'
import { schema } from '../../db/index.ts'
import type { KeycloakAdmin } from '../../keycloak/admin.ts'
import { ORG_ROLES, type OrgRole, SECURITY } from '../../plugins/auth.ts'
import { type Actor, authorize, scopeOf } from '../../plugins/authz.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { audit, auditEntries, createOrganization, memberOf, ownerCount, pendingInvitations, requireSeat, seatUsage, sharingGroups, listMembers } from './service.ts'

const Role = Type.Union(ORG_ROLES.map((r) => Type.Literal(r)))
const InviteRole = Type.Union([Type.Literal('admin'), Type.Literal('manager'), Type.Literal('member')])
const OrgParams = Type.Object({ orgId: Type.String({ format: 'uuid' }) })
const MemberParams = Type.Object({ orgId: Type.String({ format: 'uuid' }), userId: Type.String({ format: 'uuid' }) })
const Nullable = <T extends ReturnType<typeof Type.String>>(t: T) => Type.Union([t, Type.Null()])

const Member = Type.Object(
  {
    id: Type.String({ description: 'Keycloak user id' }),
    email: Nullable(Type.String()),
    name: Nullable(Type.String()),
    role: Role,
    active: Type.Boolean(),
    lastSeenAt: Type.Union([Type.Integer(), Type.Null()], { description: 'Unix ms' }),
  },
  { description: 'A member of an organization' },
)

const Invitation = Type.Object({
  id: Type.String(),
  email: Type.String(),
  role: InviteRole,
  createdAt: Type.Integer({ description: 'Unix ms' }),
})

const Seats = Type.Object({
  limit: Type.Union([Type.Integer(), Type.Null()], { description: 'From the plan; `null` is unlimited' }),
  members: Type.Integer(),
  pending: Type.Integer({ description: 'Invitations not yet accepted; they count against the limit' }),
})

const OrgInfo = Type.Object({ id: Type.String(), name: Type.String(), role: Role, seats: Seats })

export const orgRoutes: FastifyPluginAsyncTypebox = async (app) => {
  const keycloak = (): KeycloakAdmin => {
    if (!app.keycloakAdmin) {
      throw new HttpProblem(503, 'org_admin_unavailable', 'organization management is not set up: the API has no Keycloak admin credentials')
    }
    return app.keycloakAdmin
  }
  const database = () => {
    if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
    return app.db
  }

  /** The caller as a member of `orgId`; 403 for anyone else. */
  const actorIn = (request: { user: NonNullable<import('fastify').FastifyRequest['user']> }, orgId: string): Actor => {
    if (!request.user.orgs[orgId]) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
    return { user: request.user, orgId }
  }
  const seatLimit = async (user: Actor['user'], orgId: string) => {
    const issued = await app.licensing.resolve(database(), user)
    return issued.orgId === orgId ? issued.seats : null
  }

  app.post(
    '/orgs',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Creates an organization; the caller becomes its owner',
        description: 'Sign in again (or refresh the token) afterwards: the organization appears in the token\'s groups.',
        security: SECURITY,
        body: Type.Object({ name: Type.String({ minLength: 1, maxLength: 100 }) }),
        response: { 201: Type.Object({ id: Type.String(), name: Type.String() }), ...problems(401, 403, 502, 503) },
      },
    },
    async (request, reply) => {
      const db = database()
      const kc = keycloak()
      await app.licensing.require(db, request.user!, 'org')
      const org = await createOrganization(db, kc, request.user!, request.body.name.trim())
      return reply.code(201).send(org)
    },
  )

  app.get(
    '/orgs/:orgId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'The organization, the caller\'s role and the seats in use',
        security: SECURITY,
        params: OrgParams,
        response: { 200: OrgInfo, ...problems(401, 403, 404, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const db = database()
      const [org] = await db.select().from(schema.organization).where(eq(schema.organization.id, orgId))
      if (!org) throw new HttpProblem(404, 'not_found', 'organization not found')
      const usage = await seatUsage(db, orgId)
      return { id: org.id, name: org.name, role: actor.user.orgs[orgId]!.role, seats: { limit: await seatLimit(actor.user, orgId), ...usage } }
    },
  )

  app.delete(
    '/orgs/:orgId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Deletes the organization and its memberships (the members\' own data stays)',
        security: SECURITY,
        params: OrgParams,
        response: { 204: Type.Null(), ...problems(401, 403, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'org.delete', { orgId })
      const db = database()
      await keycloak().deleteOrganization(orgId)
      await db.delete(schema.organization).where(eq(schema.organization.id, orgId))
      return reply.code(204).send(null)
    },
  )

  app.get(
    '/orgs/:orgId/members',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Members the caller may see: everyone for owners and admins, the own groups for managers, only themselves for members',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(Member), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const db = database()
      const scope = scopeOf(actor, 'member.read')
      if (scope === 'org') return listMembers(db, orgId)
      if (scope === 'group') {
        const ids = await sharingGroups(db, orgId, actor.user.orgs[orgId]!.groups)
        return listMembers(db, orgId, { userIds: [...new Set([actor.user.id, ...ids])] })
      }
      if (scope === 'own') return listMembers(db, orgId, { userIds: [actor.user.id] })
      throw new HttpProblem(403, 'forbidden', 'not allowed: member.read')
    },
  )

  app.patch(
    '/orgs/:orgId/members/:userId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Changes a member\'s role (owners only) or deactivates or reactivates their account',
        description:
          'Deactivating disables the person\'s Keycloak account, so they cannot sign in anywhere; their data is kept. ' +
          'It is refused for people who are in other organizations too (remove them from this one instead). ' +
          'The last owner cannot be demoted.',
        security: SECURITY,
        params: MemberParams,
        body: Type.Object({ role: Type.Optional(Role), active: Type.Optional(Type.Boolean()) }, { minProperties: 1 }),
        response: { 200: Member, ...problems(400, 401, 403, 404, 409, 502, 503) },
      },
    },
    async (request) => {
      const { orgId, userId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const db = database()
      const target = await memberOf(db, orgId, userId)
      if (!target) throw new HttpProblem(404, 'not_found', 'not a member of this organization')
      const kc = keycloak()
      const { role, active } = request.body

      if (role !== undefined && role !== target.role) {
        authorize(actor, 'role.assign', { orgId })
        if (target.role === 'owner' && (await ownerCount(db, orgId)) <= 1) {
          throw new HttpProblem(409, 'last_owner', 'an organization needs at least one owner; make someone else an owner first')
        }
        await kc.setRole(orgId, userId, role)
        await db.update(schema.kcOrgMember).set({ role }).where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.userId, userId)))
        await audit(db, { orgId, actorId: actor.user.id, action: 'member.role', target: userId, detail: { from: target.role, to: role } })
      }

      if (active !== undefined && active !== target.active) {
        authorize(actor, 'member.manage', { orgId })
        if (target.role === 'owner' && actor.user.orgs[orgId]!.role !== 'owner') {
          throw new HttpProblem(403, 'forbidden', 'only an owner can change an owner')
        }
        if (userId === actor.user.id) throw new HttpProblem(409, 'self', 'you cannot deactivate yourself')
        const [others] = await db
          .select({ n: sql<number>`count(*)::int` })
          .from(schema.kcOrgMember)
          .where(and(eq(schema.kcOrgMember.userId, userId), ne(schema.kcOrgMember.orgId, orgId)))
        if (!active && others!.n > 0) {
          throw new HttpProblem(409, 'member_in_other_orgs', 'this person is in other organizations too; remove them from this one instead')
        }
        await kc.setEnabled(userId, active)
        await db.update(schema.kcUser).set({ active }).where(eq(schema.kcUser.id, userId))
        await audit(db, { orgId, actorId: actor.user.id, action: active ? 'member.reactivate' : 'member.deactivate', target: userId })
      }
      return (await memberOf(db, orgId, userId))!
    },
  )

  app.delete(
    '/orgs/:orgId/members/:userId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Removes a member from the organization (their account and their own data stay)',
        security: SECURITY,
        params: MemberParams,
        response: { 204: Type.Null(), ...problems(401, 403, 404, 409, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId, userId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const db = database()
      const target = await memberOf(db, orgId, userId)
      if (!target) throw new HttpProblem(404, 'not_found', 'not a member of this organization')
      // People may leave an organization themselves; removing others takes member.manage.
      if (userId !== actor.user.id) authorize(actor, 'member.manage', { orgId })
      if (target.role === 'owner') {
        if (userId !== actor.user.id && actor.user.orgs[orgId]!.role !== 'owner') {
          throw new HttpProblem(403, 'forbidden', 'only an owner can remove an owner')
        }
        if ((await ownerCount(db, orgId)) <= 1) {
          throw new HttpProblem(409, 'last_owner', 'an organization needs at least one owner; make someone else an owner first, or delete the organization')
        }
      }
      await keycloak().removeMember(orgId, userId)
      await db.transaction(async (tx) => {
        await tx.delete(schema.kcOrgMember).where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.userId, userId)))
        await tx.execute(sql`delete from kc_group_member where user_id = ${userId} and group_id in (select id from kc_group where org_id = ${orgId})`)
      })
      await audit(db, { orgId, actorId: actor.user.id, action: userId === actor.user.id ? 'member.leave' : 'member.remove', target: userId })
      return reply.code(204).send(null)
    },
  )

  app.get(
    '/orgs/:orgId/invitations',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Invitations that have not been accepted yet',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(Invitation), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'member.invite', { orgId })
      const rows = await pendingInvitations(database(), orgId)
      return rows.map((r) => ({ id: r.id, email: r.email, role: r.role as 'admin' | 'manager' | 'member', createdAt: r.createdAt }))
    },
  )

  app.post(
    '/orgs/:orgId/invitations',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Invites a person by email',
        description:
          'Keycloak sends the invitation. The person gets the role when they sign in with the invited address, which must be verified. ' +
          'Only owners can invite admins and managers. A pending invitation uses a seat.',
        security: SECURITY,
        params: OrgParams,
        body: Type.Object({ email: Type.String({ format: 'email', maxLength: 254 }), role: Type.Optional(InviteRole) }),
        response: { 201: Invitation, ...problems(400, 401, 403, 409, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'member.invite', { orgId })
      const role = request.body.role ?? 'member'
      if (role !== 'member') authorize(actor, 'role.assign', { orgId })
      const db = database()
      const kc = keycloak()
      const email = request.body.email.trim().toLowerCase()

      const [existing] = await db
        .select({ id: schema.kcUser.id })
        .from(schema.kcUser)
        .innerJoin(schema.kcOrgMember, eq(schema.kcOrgMember.userId, schema.kcUser.id))
        .where(and(eq(schema.kcOrgMember.orgId, orgId), sql`lower(${schema.kcUser.email}) = ${email}`))
      if (existing) throw new HttpProblem(409, 'already_member', 'this person is already a member')
      requireSeat(await seatLimit(actor.user, orgId), await seatUsage(db, orgId))

      const [row] = await db
        .insert(schema.orgInvitation)
        .values({ orgId, email, role, invitedBy: actor.user.id, createdAt: Date.now() })
        .onConflictDoNothing()
        .returning()
      if (!row) throw new HttpProblem(409, 'already_invited', 'this address has a pending invitation')
      try {
        await kc.invite(orgId, email)
      } catch (err) {
        await db.delete(schema.orgInvitation).where(eq(schema.orgInvitation.id, row.id))
        throw err
      }
      await audit(db, { orgId, actorId: actor.user.id, action: 'member.invite', target: email, detail: { role } })
      return reply.code(201).send({ id: row.id, email, role, createdAt: row.createdAt })
    },
  )

  app.delete(
    '/orgs/:orgId/invitations/:invitationId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Withdraws an invitation',
        description: 'The person can still follow the link in the email, but gets no role and no access to organization data.',
        security: SECURITY,
        params: Type.Object({ orgId: Type.String({ format: 'uuid' }), invitationId: Type.String({ format: 'uuid' }) }),
        response: { 204: Type.Null(), ...problems(401, 403, 404, 503) },
      },
    },
    async (request, reply) => {
      const { orgId, invitationId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'member.invite', { orgId })
      const db = database()
      const [gone] = await db
        .delete(schema.orgInvitation)
        .where(and(eq(schema.orgInvitation.id, invitationId), eq(schema.orgInvitation.orgId, orgId)))
        .returning()
      if (!gone) throw new HttpProblem(404, 'not_found', 'invitation not found')
      await audit(db, { orgId, actorId: actor.user.id, action: 'invitation.withdraw', target: gone.email })
      return reply.code(204).send(null)
    },
  )

  // ------------------------------------------------------------ departments

  const GroupParams = Type.Object({ orgId: Type.String({ format: 'uuid' }), groupId: Type.String({ format: 'uuid' }) })
  const Group = Type.Object(
    {
      id: Type.String(),
      name: Type.String(),
      path: Type.String({ description: 'Below the organization, e.g. `dev/backend`' }),
      parentId: Type.Union([Type.String(), Type.Null()]),
      memberIds: Type.Array(Type.String()),
    },
    { description: 'A department' },
  )
  const prefix = (orgId: string) => `/orgs/${orgId}/groups/`

  /** Departments of the org as the API returns them. */
  const loadGroups = async (orgId: string) => {
    const db = database()
    const groups = await db.select().from(schema.kcGroup).where(eq(schema.kcGroup.orgId, orgId)).orderBy(asc(schema.kcGroup.path))
    const members = await db
      .select({ groupId: schema.kcGroupMember.groupId, userId: schema.kcGroupMember.userId })
      .from(schema.kcGroupMember)
      .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
      .where(eq(schema.kcGroup.orgId, orgId))
    const byPath = new Map(groups.map((g) => [g.path, g.id]))
    return groups.map((g) => {
      const path = g.path.slice(prefix(orgId).length)
      const parent = path.includes('/') ? byPath.get(`${prefix(orgId)}${path.slice(0, path.lastIndexOf('/'))}`) ?? null : null
      return {
        id: g.id,
        name: path.slice(path.lastIndexOf('/') + 1),
        path,
        parentId: parent,
        memberIds: members.filter((m) => m.groupId === g.id).map((m) => m.userId),
      }
    })
  }

  app.get(
    '/orgs/:orgId/groups',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Departments: all of them for owners and admins, the caller\'s own for everyone else',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(Group), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const all = await loadGroups(orgId)
      if (scopeOf(actor, 'group.manage') === 'org') return all
      const mine = new Set(actor.user.orgs[orgId]!.groups.map((p) => p.slice(prefix(orgId).length)))
      return all.filter((g) => mine.has(g.path))
    },
  )

  app.post(
    '/orgs/:orgId/groups',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Creates a department, optionally below another one',
        security: SECURITY,
        params: OrgParams,
        body: Type.Object({
          name: Type.String({ minLength: 1, maxLength: 60, pattern: '^[^/\\s][^/]*$', description: 'No slash; not starting with a space' }),
          parentId: Type.Optional(Type.String({ format: 'uuid' })),
        }),
        response: { 201: Group, ...problems(400, 401, 403, 404, 409, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'group.manage', { orgId })
      const db = database()
      const kc = keycloak()
      const name = request.body.name.trim()
      let parentPath = ''
      if (request.body.parentId) {
        const [parent] = await db
          .select()
          .from(schema.kcGroup)
          .where(and(eq(schema.kcGroup.id, request.body.parentId), eq(schema.kcGroup.orgId, orgId)))
        if (!parent) throw new HttpProblem(404, 'not_found', 'parent department not found')
        parentPath = `${parent.path.slice(prefix(orgId).length)}/`
      }
      const path = `${parentPath}${name}`
      const [clash] = await db.select().from(schema.kcGroup).where(eq(schema.kcGroup.path, `${prefix(orgId)}${path}`))
      if (clash) throw new HttpProblem(409, 'already_exists', 'a department with this name already exists here')
      const id = await kc.createGroup(orgId, path.split('/'))
      await db.insert(schema.kcGroup).values({ id, orgId, path: `${prefix(orgId)}${path}` })
      await audit(db, { orgId, actorId: actor.user.id, action: 'group.create', target: path })
      return reply.code(201).send({ id, name, path, parentId: request.body.parentId ?? null, memberIds: [] })
    },
  )

  app.delete(
    '/orgs/:orgId/groups/:groupId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Deletes a department and the departments below it',
        security: SECURITY,
        params: GroupParams,
        response: { 204: Type.Null(), ...problems(401, 403, 404, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId, groupId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'group.manage', { orgId })
      const db = database()
      const [group] = await db.select().from(schema.kcGroup).where(and(eq(schema.kcGroup.id, groupId), eq(schema.kcGroup.orgId, orgId)))
      if (!group) throw new HttpProblem(404, 'not_found', 'department not found')
      await keycloak().deleteGroup(groupId)
      // Keycloak removes the subgroups too; the mirror follows (members cascade).
      await db
        .delete(schema.kcGroup)
        .where(and(eq(schema.kcGroup.orgId, orgId), sql`(${schema.kcGroup.path} = ${group.path} or ${schema.kcGroup.path} like ${`${group.path.replace(/[\\%_]/g, '\\$&')}/%`})`))
      await audit(db, { orgId, actorId: actor.user.id, action: 'group.delete', target: group.path.slice(prefix(orgId).length) })
      return reply.code(204).send(null)
    },
  )

  for (const method of ['PUT', 'DELETE'] as const) {
    app.route({
      method,
      url: '/orgs/:orgId/groups/:groupId/members/:userId',
      schema: {
        tags: ['organizations'],
        summary: method === 'PUT' ? 'Puts a member into a department' : 'Takes a member out of a department',
        security: SECURITY,
        params: Type.Object({ orgId: Type.String({ format: 'uuid' }), groupId: Type.String({ format: 'uuid' }), userId: Type.String({ format: 'uuid' }) }),
        response: { 204: Type.Null(), ...problems(401, 403, 404, 502, 503) },
      },
      handler: async (request, reply) => {
        const { orgId, groupId, userId } = request.params as { orgId: string; groupId: string; userId: string }
        const actor = actorIn({ user: request.user! }, orgId)
        authorize(actor, 'group.manage', { orgId })
        const db = database()
        const [group] = await db.select().from(schema.kcGroup).where(and(eq(schema.kcGroup.id, groupId), eq(schema.kcGroup.orgId, orgId)))
        if (!group) throw new HttpProblem(404, 'not_found', 'department not found')
        if (!(await memberOf(db, orgId, userId))) throw new HttpProblem(404, 'not_found', 'not a member of this organization')
        const kc = keycloak()
        if (method === 'PUT') {
          await kc.addToGroup(userId, groupId)
          await db.insert(schema.kcGroupMember).values({ groupId, userId }).onConflictDoNothing()
        } else {
          await kc.removeFromGroup(userId, groupId)
          await db.delete(schema.kcGroupMember).where(and(eq(schema.kcGroupMember.groupId, groupId), eq(schema.kcGroupMember.userId, userId)))
        }
        await audit(db, {
          orgId,
          actorId: actor.user.id,
          action: method === 'PUT' ? 'group.add' : 'group.remove',
          target: userId,
          detail: { group: group.path.slice(prefix(orgId).length) },
        })
        return reply.code(204).send(null)
      },
    })
  }

  app.get(
    '/orgs/:orgId/audit',
    {
      schema: {
        tags: ['organizations'],
        summary: 'The audit log, newest first',
        security: SECURITY,
        params: OrgParams,
        querystring: Type.Object({
          before: Type.Optional(Type.Integer({ minimum: 1, description: 'Entries older than this id' })),
          limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 200, default: 50 })),
        }),
        response: {
          200: Type.Array(
            Type.Object({
              id: Type.Integer(),
              at: Type.Integer({ description: 'Unix ms' }),
              actorId: Type.Union([Type.String(), Type.Null()]),
              action: Type.String(),
              target: Type.Union([Type.String(), Type.Null()]),
              detail: Type.Record(Type.String(), Type.Unknown()),
            }),
          ),
          ...problems(401, 403, 503),
        },
      },
    },
    async (request) => {
      const { orgId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'audit.read', { orgId })
      const rows = await auditEntries(database(), orgId, { before: request.query.before, limit: request.query.limit ?? 50 })
      return rows.map((r) => ({ id: r.id, at: r.at, actorId: r.actorId, action: r.action, target: r.target, detail: r.detail as Record<string, unknown> }))
    },
  )
}

export type { OrgRole }
