// Organizations and their members (SPEC §5.9). Only people with the right
// role in the organization may call these (permissions.yaml); changes go to
// Keycloak first (src/keycloak/admin.ts), then to the mirror tables.

import { randomUUID } from 'node:crypto'
import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { and, asc, eq, inArray, isNull, ne, sql } from 'drizzle-orm'
import { Type } from 'typebox'
import { type Db, schema } from '../../db/index.ts'
import type { KeycloakAdmin } from '../../keycloak/admin.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { CATALOG, cleanPermissions, covers, isBuiltinRole, type Permissions, SCOPES } from '../../plugins/permissions.ts'
import { type Actor, authorize, can, scopeOf } from '../../plugins/authz.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { PRESENCE_STATES } from '../../plugins/presence.ts'
import { departmentsOf, effective, loadPolicies, orgWorkspaceId } from '../sync/policy.ts'
import { reconcileOrganization } from './reconcile.ts'
import { forgetRoles, loadRoles, roleOf } from './roles.ts'
import { freeBusy } from '../../meetings/freebusy.ts'
import { roomBookings } from '../../meetings/rooms.ts'
import { audit, auditEntries, avatarsOf, createOrganization, deleteOrganization, memberOf, ownerCount, pendingInvitations, requireSeat, seatUsage, sharingGroups, listMembers } from './service.ts'

/** A role: a built-in one's name (`admin`) or the id of one the organization made. */
const Role = Type.String({ minLength: 1, maxLength: 64, pattern: '^[A-Za-z0-9-]+$' })
const InviteRole = Role
const OrgParams = Type.Object({ orgId: Type.String({ format: 'uuid' }) })
const MemberParams = Type.Object({ orgId: Type.String({ format: 'uuid' }), userId: Type.String({ format: 'uuid' }) })
const Nullable = <T extends ReturnType<typeof Type.String>>(t: T) => Type.Union([t, Type.Null()])

/**
 * Whether the actor may give `role` to someone: it must exist, only an owner
 * makes owners, and nobody hands out permissions they do not hold themselves.
 */
async function mayGiveRole(db: Db, actor: Actor, orgId: string, role: string) {
  const target = await roleOf(db, orgId, role)
  if (!target) throw new HttpProblem(404, 'not_found', 'no such role')
  const mine = actor.user.orgs[orgId]!
  if (role === 'owner' && mine.role !== 'owner') throw new HttpProblem(403, 'forbidden', 'only an owner can make owners')
  if (!covers(mine.permissions, target.permissions)) {
    throw new HttpProblem(403, 'forbidden', 'this role has permissions you do not have yourself')
  }
}

/** The departments and the job description must belong to the organization. */
async function checkDepartmentsAndJob(db: Db, orgId: string, groupIds: readonly string[], jobId: string | null) {
  if (groupIds.length) {
    const found = await db.select({ id: schema.kcGroup.id }).from(schema.kcGroup).where(and(eq(schema.kcGroup.orgId, orgId), inArray(schema.kcGroup.id, [...groupIds])))
    if (found.length !== groupIds.length) throw new HttpProblem(404, 'not_found', 'department not found')
  }
  if (jobId) {
    const [job] = await db.select({ id: schema.orgJob.id }).from(schema.orgJob).where(and(eq(schema.orgJob.id, jobId), eq(schema.orgJob.orgId, orgId)))
    if (!job) throw new HttpProblem(404, 'not_found', 'no such job description')
  }
}

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
  groupIds: Type.Array(Type.String(), { description: 'Departments the person is put into when they join' }),
  jobId: Type.Union([Type.String(), Type.Null()], { description: 'The job description they get when they join' }),
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

  // On the cloud every organization route needs a plan with organizations.
  app.addHook('preHandler', async (request) => {
    if (request.user && app.licensing.enforced) await app.licensing.require(database(), request.user, 'org')
  })

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
        summary: 'Deletes the organization, its memberships and its shared workspace (the members\' personal data stays)',
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
      await deleteOrganization(db, orgId)
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
        await mayGiveRole(db, actor, orgId, role)
        // Nor may someone take a role from a person who holds more than they do.
        const current = await roleOf(db, orgId, target.role)
        if (current && !covers(actor.user.orgs[orgId]!.permissions, current.permissions)) {
          throw new HttpProblem(403, 'forbidden', 'this person has permissions you do not have')
        }
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
      // People may leave an organization themselves; removing others takes member.remove.
      if (userId !== actor.user.id) authorize(actor, 'member.remove', { orgId })
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
      return rows.map((r) => ({ id: r.id, email: r.email, role: r.role as 'admin' | 'manager' | 'member', groupIds: r.groupIds, jobId: r.jobId, createdAt: r.createdAt }))
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
        body: Type.Object({
          email: Type.String({ format: 'email', maxLength: 254 }),
          role: Type.Optional(InviteRole),
          groupIds: Type.Optional(Type.Array(Type.String({ format: 'uuid' }), { maxItems: 50, description: 'Departments to put the person into when they join' })),
          jobId: Type.Optional(Nullable(Type.String({ format: 'uuid' }))),
        }),
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
      await mayGiveRole(db, actor, orgId, role)
      const kc = keycloak()
      const email = request.body.email.trim().toLowerCase()

      const [existing] = await db
        .select({ id: schema.kcUser.id })
        .from(schema.kcUser)
        .innerJoin(schema.kcOrgMember, eq(schema.kcOrgMember.userId, schema.kcUser.id))
        .where(and(eq(schema.kcOrgMember.orgId, orgId), sql`lower(${schema.kcUser.email}) = ${email}`))
      if (existing) throw new HttpProblem(409, 'already_member', 'this person is already a member')
      requireSeat(await seatLimit(actor.user, orgId), await seatUsage(db, orgId))

      const groupIds = [...new Set(request.body.groupIds ?? [])]
      const jobId = request.body.jobId ?? null
      await checkDepartmentsAndJob(db, orgId, groupIds, jobId)
      const [row] = await db
        .insert(schema.orgInvitation)
        .values({ orgId, email, role, groupIds, jobId, invitedBy: actor.user.id, createdAt: Date.now() })
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
      return reply.code(201).send({ id: row.id, email, role, groupIds, jobId, createdAt: row.createdAt })
    },
  )

  app.patch(
    '/orgs/:orgId/invitations/:invitationId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Changes the departments and job description a pending invitation gives',
        security: SECURITY,
        params: Type.Object({ orgId: Type.String({ format: 'uuid' }), invitationId: Type.String({ format: 'uuid' }) }),
        body: Type.Object(
          {
            groupIds: Type.Optional(Type.Array(Type.String({ format: 'uuid' }), { maxItems: 50 })),
            jobId: Type.Optional(Nullable(Type.String({ format: 'uuid' }))),
          },
          { minProperties: 1 },
        ),
        response: { 200: Invitation, ...problems(400, 401, 403, 404, 503) },
      },
    },
    async (request) => {
      const { orgId, invitationId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'member.invite', { orgId })
      const db = database()
      const groupIds = request.body.groupIds ? [...new Set(request.body.groupIds)] : undefined
      await checkDepartmentsAndJob(db, orgId, groupIds ?? [], request.body.jobId ?? null)
      const [row] = await db
        .update(schema.orgInvitation)
        .set({ ...(groupIds ? { groupIds } : {}), ...(request.body.jobId !== undefined ? { jobId: request.body.jobId } : {}) })
        .where(and(eq(schema.orgInvitation.id, invitationId), eq(schema.orgInvitation.orgId, orgId), isNull(schema.orgInvitation.acceptedAt)))
        .returning()
      if (!row) throw new HttpProblem(404, 'not_found', 'invitation not found')
      return { id: row.id, email: row.email, role: row.role as 'admin' | 'manager' | 'member', groupIds: row.groupIds, jobId: row.jobId, createdAt: row.createdAt }
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

  app.patch(
    '/orgs/:orgId/groups/:groupId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Renames a department (the ones below it follow)',
        description: 'Refused while rules are set for the department or one below it: they name it by its path. Remove them first.',
        security: SECURITY,
        params: GroupParams,
        body: Type.Object({ name: Type.String({ minLength: 1, maxLength: 60, pattern: '^[^/\\s][^/]*$', description: 'No slash; not starting with a space' }) }),
        response: { 200: Group, ...problems(400, 401, 403, 404, 409, 502, 503) },
      },
    },
    async (request) => {
      const { orgId, groupId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'group.manage', { orgId })
      const db = database()
      const [group] = await db.select().from(schema.kcGroup).where(and(eq(schema.kcGroup.id, groupId), eq(schema.kcGroup.orgId, orgId)))
      if (!group) throw new HttpProblem(404, 'not_found', 'department not found')
      const oldPath = group.path.slice(prefix(orgId).length)
      const name = request.body.name.trim()
      const newPath = `${oldPath.includes('/') ? `${oldPath.slice(0, oldPath.lastIndexOf('/'))}/` : ''}${name}`
      if (newPath === oldPath) return (await loadGroups(orgId)).find((g) => g.id === groupId)!
      const [clash] = await db.select().from(schema.kcGroup).where(eq(schema.kcGroup.path, `${prefix(orgId)}${newPath}`))
      if (clash) throw new HttpProblem(409, 'already_exists', 'a department with this name already exists here')
      // Rules name their department by path: they would point nowhere.
      const workspace = await orgWorkspaceId(db, orgId)
      if (workspace) {
        const rules = await db
          .select({ fields: schema.syncRow.fields, deleted: schema.syncRow.deleted })
          .from(schema.syncRow)
          .where(and(eq(schema.syncRow.workspaceId, workspace), eq(schema.syncRow.tableName, 'org_policy')))
        const named = rules.some((r) => {
          const path = (r.fields as Record<string, { v: unknown }>).groupPath?.v
          return r.deleted === null && typeof path === 'string' && (path === oldPath || path.startsWith(`${oldPath}/`))
        })
        if (named) throw new HttpProblem(409, 'department_has_rules', 'rules are set for this department; remove them before renaming it')
      }
      await keycloak().renameGroup(groupId, name)
      // The ones below follow in Keycloak; the mirror's paths follow too.
      const like = `${group.path.replace(/[\\%_]/g, '\\$&')}/%`
      await db
        .update(schema.kcGroup)
        .set({ path: sql`${`${prefix(orgId)}${newPath}`} || substr(${schema.kcGroup.path}, ${group.path.length + 1})` })
        .where(and(eq(schema.kcGroup.orgId, orgId), sql`(${schema.kcGroup.path} = ${group.path} or ${schema.kcGroup.path} like ${like})`))
      await audit(db, { orgId, actorId: actor.user.id, action: 'group.rename', target: newPath, detail: { from: oldPath } })
      return (await loadGroups(orgId)).find((g) => g.id === groupId)!
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
      authorize(actor, 'group.delete', { orgId })
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
    '/orgs/:orgId/people',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Names of everyone in the organization (for lists that show other people)',
        description: 'Display name and profile picture, so presence, locations and timesheets can say who is who. No email addresses.',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(Type.Object({ id: Type.String(), name: Type.Union([Type.String(), Type.Null()]), avatar: Type.Union([Type.String(), Type.Null()], { description: 'Profile picture as a data URL' }) })), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      actorIn({ user: request.user! }, orgId)
      const db = database()
      const rows = await listMembers(db, orgId)
      const pictures = await avatarsOf(db, rows.map((m) => m.id))
      return rows.map((m) => ({ id: m.id, name: m.name, avatar: pictures.get(m.id) ?? null }))
    },
  )

  // ------------------------------------------------------ job descriptions
  const Job = Type.Object({
    id: Type.String(),
    name: Type.String(),
    description: Type.String(),
    people: Type.Integer({ description: 'How many members have it' }),
  })
  const JobParams = Type.Object({ orgId: Type.String({ format: 'uuid' }), jobId: Type.String({ format: 'uuid' }) })
  const JobBody = Type.Object({ name: Type.String({ minLength: 1, maxLength: 100 }), description: Type.Optional(Type.String({ maxLength: 500 })) })

  async function jobsOf(db: ReturnType<typeof database>, orgId: string) {
    const rows = await db
      .select({ id: schema.orgJob.id, name: schema.orgJob.name, description: schema.orgJob.description, people: sql<number>`count(${schema.kcOrgMember.userId})::int` })
      .from(schema.orgJob)
      .leftJoin(schema.kcOrgMember, eq(schema.kcOrgMember.jobId, schema.orgJob.id))
      .where(eq(schema.orgJob.orgId, orgId))
      .groupBy(schema.orgJob.id)
      .orderBy(asc(sql`lower(${schema.orgJob.name})`))
    return rows
  }
  const nameTaken = (err: unknown) => (err as { cause?: { code?: string } }).cause?.code === '23505'

  app.get(
    '/orgs/:orgId/jobs',
    {
      schema: {
        tags: ['organizations'],
        summary: 'The job descriptions of the organization',
        description: 'Every member may ask.',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(Job), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      actorIn({ user: request.user! }, orgId)
      return jobsOf(database(), orgId)
    },
  )

  app.post(
    '/orgs/:orgId/jobs',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Defines a job description (those who manage members)',
        security: SECURITY,
        params: OrgParams,
        body: JobBody,
        response: { 201: Job, ...problems(400, 401, 403, 409, 503) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'job.edit', { orgId })
      const id = randomUUID()
      try {
        await database().insert(schema.orgJob).values({ id, orgId, name: request.body.name.trim(), description: request.body.description?.trim() ?? '' })
      } catch (err) {
        if (nameTaken(err)) throw new HttpProblem(409, 'conflict', 'a job description with this name exists')
        throw err
      }
      const job = (await jobsOf(database(), orgId)).find((j) => j.id === id)!
      return reply.code(201).send(job)
    },
  )

  app.patch(
    '/orgs/:orgId/jobs/:jobId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Changes a job description (those who manage members)',
        security: SECURITY,
        params: JobParams,
        body: Type.Partial(JobBody, { minProperties: 1 }),
        response: { 200: Job, ...problems(400, 401, 403, 404, 409, 503) },
      },
    },
    async (request) => {
      const { orgId, jobId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'job.edit', { orgId })
      const db = database()
      const set: { name?: string; description?: string } = {}
      if (request.body.name !== undefined) set.name = request.body.name.trim()
      if (request.body.description !== undefined) set.description = request.body.description.trim()
      try {
        const updated = await db.update(schema.orgJob).set(set).where(and(eq(schema.orgJob.id, jobId), eq(schema.orgJob.orgId, orgId))).returning({ id: schema.orgJob.id })
        if (updated.length === 0) throw new HttpProblem(404, 'not_found', 'no such job description')
      } catch (err) {
        if (nameTaken(err)) throw new HttpProblem(409, 'conflict', 'a job description with this name exists')
        throw err
      }
      return (await jobsOf(db, orgId)).find((j) => j.id === jobId)!
    },
  )

  app.delete(
    '/orgs/:orgId/jobs/:jobId',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Removes a job description; the people who had it have none then',
        security: SECURITY,
        params: JobParams,
        response: { 204: Type.Null(), ...problems(401, 403, 404, 503) },
      },
    },
    async (request, reply) => {
      const { orgId, jobId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'job.delete', { orgId })
      const removed = await database().delete(schema.orgJob).where(and(eq(schema.orgJob.id, jobId), eq(schema.orgJob.orgId, orgId))).returning({ id: schema.orgJob.id })
      if (removed.length === 0) throw new HttpProblem(404, 'not_found', 'no such job description')
      return reply.code(204).send(null)
    },
  )

  const DirectoryEntry = Type.Object({
    id: Type.String(),
    name: Nullable(Type.String()),
    email: Nullable(Type.String()),
    avatar: Nullable(Type.String({ description: 'Profile picture as a data URL' })),
    departments: Type.Array(Type.String(), { description: 'Department paths without the prefix, e.g. `dev/backend`' }),
    jobId: Nullable(Type.String()),
    jobTitle: Nullable(Type.String({ description: 'Name of the job description' })),
    jobDescription: Nullable(Type.String()),
  })

  app.get(
    '/orgs/:orgId/directory',
    {
      schema: {
        tags: ['organizations'],
        summary: 'The coworkers: name, email, departments and job description',
        description: 'Every member may ask. Accounts that were deactivated are left out.',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(DirectoryEntry), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      actorIn({ user: request.user! }, orgId)
      const db = database()
      const members = (await listMembers(db, orgId)).filter((m) => m.active)
      const jobs = new Map((await jobsOf(db, orgId)).map((j) => [j.id, j]))
      const held = await db
        .select({ userId: schema.kcOrgMember.userId, jobId: schema.kcOrgMember.jobId })
        .from(schema.kcOrgMember)
        .where(eq(schema.kcOrgMember.orgId, orgId))
      const jobOf = new Map(held.map((h) => [h.userId, h.jobId]))
      const groups = await db
        .select({ userId: schema.kcGroupMember.userId, path: schema.kcGroup.path })
        .from(schema.kcGroupMember)
        .innerJoin(schema.kcGroup, eq(schema.kcGroup.id, schema.kcGroupMember.groupId))
        .where(eq(schema.kcGroup.orgId, orgId))
      const head = `/orgs/${orgId}/groups/`
      const departments = new Map<string, string[]>()
      for (const g of groups) {
        if (!g.path.startsWith(head)) continue
        departments.set(g.userId, [...(departments.get(g.userId) ?? []), g.path.slice(head.length)].sort())
      }
      const pictures = await avatarsOf(db, members.map((m) => m.id))
      return members.map((m) => {
        const job = jobs.get(jobOf.get(m.id) ?? '')
        return { id: m.id, name: m.name, email: m.email, avatar: pictures.get(m.id) ?? null, departments: departments.get(m.id) ?? [], jobId: job?.id ?? null, jobTitle: job?.name ?? null, jobDescription: job?.description || null }
      })
    },
  )

  app.put(
    '/orgs/:orgId/members/:userId/job',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Gives a member a job description from the list (those who manage members; people do not choose their own)',
        security: SECURITY,
        params: MemberParams,
        body: Type.Object({ jobId: Nullable(Type.String({ format: 'uuid' })) }),
        response: { 204: Type.Null(), ...problems(401, 403, 404, 503) },
      },
    },
    async (request, reply) => {
      const { orgId, userId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'member.manage', { orgId })
      const db = database()
      if (!(await memberOf(db, orgId, userId))) throw new HttpProblem(404, 'not_found', 'not a member of this organization')
      const { jobId } = request.body
      if (jobId) {
        const [job] = await db.select({ id: schema.orgJob.id }).from(schema.orgJob).where(and(eq(schema.orgJob.id, jobId), eq(schema.orgJob.orgId, orgId)))
        if (!job) throw new HttpProblem(404, 'not_found', 'no such job description')
      }
      await db.update(schema.kcOrgMember).set({ jobId }).where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.userId, userId)))
      return reply.code(204).send(null)
    },
  )

  // ------------------------------------------------------------------ roles
  const RoleParams = Type.Object({ orgId: Type.String({ format: 'uuid' }), roleKey: Role })
  const PermissionMap = Type.Record(Type.String(), Type.String(), {
    description: 'Action → the widest scope it is allowed for (`own`, `group` or `org`; an action missing is denied)',
  })
  const RoleOut = Type.Object({
    key: Type.String({ description: 'A built-in role\'s name (`admin`) or the id of one the organization made' }),
    name: Type.String(),
    description: Type.String(),
    builtin: Type.Boolean(),
    locked: Type.Boolean({ description: 'The owner role cannot be changed' }),
    people: Type.Integer({ description: 'How many members have it' }),
    permissions: Type.Union([PermissionMap, Type.Null()]),
  })
  const CatalogEntry = Type.Object({
    action: Type.String(),
    resource: Type.String(),
    kind: Type.Union([Type.Literal('view'), Type.Literal('edit'), Type.Literal('delete')]),
    scopes: Type.Array(Type.Union(SCOPES.map((x) => Type.Literal(x)))),
  })
  const RoleBody = Type.Object({
    name: Type.String({ minLength: 1, maxLength: 60 }),
    description: Type.Optional(Type.String({ maxLength: 300 })),
    permissions: PermissionMap,
  })

  async function rolesOut(orgId: string, actor: Actor) {
    const db = database()
    const counts = await db
      .select({ role: schema.kcOrgMember.role, n: sql<number>`count(*)::int` })
      .from(schema.kcOrgMember)
      .where(eq(schema.kcOrgMember.orgId, orgId))
      .groupBy(schema.kcOrgMember.role)
    const people = new Map(counts.map((c) => [c.role, c.n]))
    const seeAll = can(actor, 'role.view', { orgId })
    return (await loadRoles(db, orgId)).map((r) => ({
      key: r.key,
      name: r.name,
      description: r.description,
      builtin: r.builtin,
      locked: r.locked,
      people: people.get(r.key) ?? 0,
      // What a role allows is for those who look after roles, and for their own.
      permissions: seeAll || actor.user.orgs[orgId]!.role === r.key ? r.permissions : null,
    }))
  }

  app.get(
    '/orgs/:orgId/roles',
    {
      schema: {
        tags: ['organizations'],
        summary: 'The roles of the organization and the permissions they are made of',
        description: 'Every member may ask for the names. What each role allows is shown to those who view roles, and each person gets their own.',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Object({ roles: Type.Array(RoleOut), catalog: Type.Array(CatalogEntry) }), ...problems(401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      return { roles: await rolesOut(orgId, actor), catalog: CATALOG }
    },
  )

  /** A role's permissions may not exceed the giver's own (nobody grants more than they hold). */
  const checkGrant = (actor: Actor, orgId: string, raw: Record<string, unknown>): Permissions => {
    let permissions: Permissions
    try {
      permissions = cleanPermissions(raw)
    } catch (err) {
      throw new HttpProblem(400, 'bad_request', (err as Error).message)
    }
    if (!covers(actor.user.orgs[orgId]!.permissions, permissions)) {
      throw new HttpProblem(403, 'forbidden', 'a role cannot have permissions you do not have yourself')
    }
    return permissions
  }

  app.post(
    '/orgs/:orgId/roles',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Creates a role with the given permissions',
        description: 'Only permissions the caller holds themselves can be given.',
        security: SECURITY,
        params: OrgParams,
        body: RoleBody,
        response: { 201: RoleOut, ...problems(400, 401, 403, 409, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'role.edit', { orgId })
      const db = database()
      const permissions = checkGrant(actor, orgId, request.body.permissions)
      const name = request.body.name.trim()
      if ((await loadRoles(db, orgId)).some((r) => r.name.toLowerCase() === name.toLowerCase())) {
        throw new HttpProblem(409, 'conflict', 'a role with this name exists')
      }
      const id = randomUUID()
      await keycloak().createRole(orgId, id)
      await db.insert(schema.orgRole).values({ id, orgId, key: null, name, description: request.body.description?.trim() ?? '', permissions })
      forgetRoles(orgId)
      await audit(db, { orgId, actorId: actor.user.id, action: 'role.create', target: name })
      return reply.code(201).send((await rolesOut(orgId, actor)).find((r) => r.key === id)!)
    },
  )

  app.patch(
    '/orgs/:orgId/roles/:roleKey',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Changes a role',
        description: 'The owner role is fixed, and nobody changes the role they hold themselves. The built-in roles keep their names.',
        security: SECURITY,
        params: RoleParams,
        body: Type.Partial(RoleBody, { minProperties: 1 }),
        response: { 200: RoleOut, ...problems(400, 401, 403, 404, 409, 503) },
      },
    },
    async (request) => {
      const { orgId, roleKey } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'role.edit', { orgId })
      const db = database()
      const role = await roleOf(db, orgId, roleKey)
      if (!role) throw new HttpProblem(404, 'not_found', 'no such role')
      if (role.locked) throw new HttpProblem(403, 'forbidden', 'the owner role cannot be changed')
      if (actor.user.orgs[orgId]!.role === roleKey) throw new HttpProblem(403, 'forbidden', 'you cannot change the role you have yourself')
      const permissions = request.body.permissions ? checkGrant(actor, orgId, request.body.permissions) : role.permissions
      // Changing what a role allows takes in everyone who has it: the changer must cover what it allowed before too.
      if (!covers(actor.user.orgs[orgId]!.permissions, role.permissions)) {
        throw new HttpProblem(403, 'forbidden', 'this role has permissions you do not have yourself')
      }
      const name = role.builtin ? role.name : (request.body.name?.trim() ?? role.name)
      if (!role.builtin && name.toLowerCase() !== role.name.toLowerCase() && (await loadRoles(db, orgId)).some((r) => r.name.toLowerCase() === name.toLowerCase())) {
        throw new HttpProblem(409, 'conflict', 'a role with this name exists')
      }
      const description = request.body.description?.trim() ?? role.description
      if (role.builtin) {
        await db
          .insert(schema.orgRole)
          .values({ id: randomUUID(), orgId, key: roleKey, name, description, permissions })
          .onConflictDoUpdate({ target: [schema.orgRole.orgId, schema.orgRole.key], targetWhere: sql`${schema.orgRole.key} IS NOT NULL`, set: { description, permissions } })
      } else {
        await db.update(schema.orgRole).set({ name, description, permissions }).where(and(eq(schema.orgRole.id, roleKey), eq(schema.orgRole.orgId, orgId)))
      }
      forgetRoles(orgId)
      await audit(db, { orgId, actorId: actor.user.id, action: 'role.update', target: name })
      return (await rolesOut(orgId, actor)).find((r) => r.key === roleKey)!
    },
  )

  app.delete(
    '/orgs/:orgId/roles/:roleKey',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Deletes a role the organization made; the people who had it become members',
        security: SECURITY,
        params: RoleParams,
        response: { 204: Type.Null(), ...problems(401, 403, 404, 409, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId, roleKey } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'role.delete', { orgId })
      const db = database()
      const role = await roleOf(db, orgId, roleKey)
      if (!role) throw new HttpProblem(404, 'not_found', 'no such role')
      if (isBuiltinRole(roleKey)) throw new HttpProblem(409, 'builtin', 'the built-in roles cannot be deleted')
      if (actor.user.orgs[orgId]!.role === roleKey) throw new HttpProblem(403, 'forbidden', 'you cannot delete the role you have yourself')
      const holders = await db
        .select({ userId: schema.kcOrgMember.userId })
        .from(schema.kcOrgMember)
        .where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.role, roleKey)))
      const kc = keycloak()
      for (const h of holders) await kc.setRole(orgId, h.userId, 'member')
      await db.update(schema.kcOrgMember).set({ role: 'member' }).where(and(eq(schema.kcOrgMember.orgId, orgId), eq(schema.kcOrgMember.role, roleKey)))
      await kc.deleteRole(orgId, roleKey)
      await db.delete(schema.orgRole).where(and(eq(schema.orgRole.id, roleKey), eq(schema.orgRole.orgId, orgId)))
      forgetRoles(orgId)
      await audit(db, { orgId, actorId: actor.user.id, action: 'role.delete', target: role.name })
      return reply.code(204).send(null)
    },
  )

  // --------------------------------------------------------------- presence
  const PresenceItem = Type.Object({
    userId: Type.String(),
    state: Type.Union(PRESENCE_STATES.map((s) => Type.Literal(s))),
    projectId: Type.Union([Type.String(), Type.Null()], { description: 'Only when the person chose to share what they work on' }),
    since: Type.Integer({ description: 'Unix ms' }),
  })

  app.put(
    '/orgs/:orgId/presence',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Reports the caller\'s presence (online, in a meeting, traveling, away)',
        description:
          'Devices report on every change and about once a minute; a report older than two and a half minutes no longer counts, and neither does one from a person with no connected device. ' +
          'Send `projectId` only if the person chose to share what they work on. `offline` removes the report.',
        security: SECURITY,
        params: OrgParams,
        body: Type.Object({
          // `tracking` is what older apps still send; it counts as online.
          state: Type.Union([...PRESENCE_STATES.map((s) => Type.Literal(s)), Type.Literal('offline'), Type.Literal('tracking')]),
          projectId: Type.Optional(Type.Union([Type.String({ format: 'uuid' }), Type.Null()])),
        }),
        response: { 204: Type.Null(), ...problems(400, 401, 403) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const { state: reported, projectId } = request.body
      const state = reported === 'tracking' ? 'online' : reported
      const changed =
        state === 'offline'
          ? app.presence.remove(actor.user.id, orgId).length > 0
          : app.presence.set(orgId, actor.user.id, state, projectId ?? null, Date.now())
      if (changed) await app.notifyPresence(orgId)
      return reply.code(204).send(null)
    },
  )

  app.get(
    '/orgs/:orgId/presence',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Who is online, tracking or away in the organization',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Type.Array(PresenceItem), ...problems(401, 403) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      const present = app.presence.list(orgId, Date.now(), (userId) => app.hub.has(userId))
      // A person whose policy hides presence is seen only by themselves.
      const workspace = await orgWorkspaceId(database(), orgId)
      const policies = workspace ? await loadPolicies(database(), workspace) : []
      if (policies.length === 0) return present
      const shown = []
      for (const p of present) {
        if (p.userId === actor.user.id) shown.push(p)
        else if (effective(policies, await departmentsOf(database(), orgId, p.userId)).presenceVisibility === 'org') shown.push(p)
      }
      return shown
    },
  )

  app.post(
    '/orgs/:orgId/reconcile',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Checks the member and department lists against Keycloak now',
        description: 'The API does this every night and when Keycloak reports a change; this is for after changes made in the Keycloak console.',
        security: SECURITY,
        params: OrgParams,
        response: {
          200: Type.Object({ members: Type.Integer(), addedMembers: Type.Integer(), removedMembers: Type.Integer(), groups: Type.Integer() }),
          ...problems(401, 403, 502, 503),
        },
      },
    },
    async (request) => {
      const { orgId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'member.manage', { orgId })
      return reconcileOrganization(database(), keycloak(), orgId)
    },
  )

  app.post(
    '/orgs/:orgId/freebusy',
    {
      schema: {
        tags: ['organizations'],
        summary: 'When people are busy (for planning a meeting)',
        description:
          'Only times, never what for: meetings the person organizes or has not declined, and approved absences. ' +
          'Every member may ask about every member. `timezone` is the zone whose days absences are read in.',
        security: SECURITY,
        params: OrgParams,
        body: Type.Object({
          userIds: Type.Array(Type.String({ format: 'uuid' }), { minItems: 1, maxItems: 50 }),
          from: Type.Integer({ description: 'Unix ms' }),
          to: Type.Integer({ description: 'Unix ms; at most 31 days after `from`' }),
          timezone: Type.Optional(Type.String({ maxLength: 64 })),
        }),
        response: {
          200: Type.Record(Type.String(), Type.Array(Type.Object({ from: Type.Integer(), to: Type.Integer(), kind: Type.Union([Type.Literal('meeting'), Type.Literal('absence')]) }))),
          ...problems(400, 401, 403, 503),
        },
      },
    },
    async (request) => {
      const { orgId } = request.params
      actorIn({ user: request.user! }, orgId)
      const { userIds, from, to, timezone } = request.body
      if (to <= from || to - from > 31 * 86_400_000) throw new HttpProblem(400, 'bad_request', '`to` must be after `from`, within 31 days')
      const workspace = await orgWorkspaceId(database(), orgId)
      if (!workspace) return Object.fromEntries(userIds.map((u) => [u, []]))
      // Only people of this organization.
      const members = new Set((await listMembers(database(), orgId)).map((m) => m.id))
      return freeBusy(database(), workspace, userIds.filter((u) => members.has(u)), from, to, timezone ?? 'UTC')
    },
  )

  app.post(
    '/orgs/:orgId/rooms/bookings',
    {
      schema: {
        tags: ['organizations'],
        summary: 'When the meeting rooms are booked (for choosing one)',
        description: 'Only times, never which meeting. Every member may ask. `to` is at most 31 days after `from`.',
        security: SECURITY,
        params: OrgParams,
        body: Type.Object({
          from: Type.Integer({ description: 'Unix ms' }),
          to: Type.Integer({ description: 'Unix ms' }),
          exclude: Type.Optional(Type.String({ format: 'uuid', description: 'A meeting to leave out: the one being changed' })),
        }),
        response: { 200: Type.Record(Type.String(), Type.Array(Type.Object({ from: Type.Integer(), to: Type.Integer() }))), ...problems(400, 401, 403, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      actorIn({ user: request.user! }, orgId)
      const { from, to, exclude } = request.body
      if (to <= from || to - from > 31 * 86_400_000) throw new HttpProblem(400, 'bad_request', '`to` must be after `from`, within 31 days')
      const workspace = await orgWorkspaceId(database(), orgId)
      return workspace ? roomBookings(database(), workspace, from, to, exclude) : {}
    },
  )

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

