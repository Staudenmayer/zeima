// The signed-in user as the API sees them: identity and org memberships.

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { eq } from 'drizzle-orm'
import { Type } from 'typebox'
import * as schema from '../../db/schema.ts'
import { OrgIdHeader, SECURITY } from '../../plugins/auth.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'

/** What a profile picture may be: a small image as a data URL (the app shrinks it first). */
export const AVATAR_MAX = 80_000
const AVATAR = /^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/]+=*$/

const Me = Type.Object(
  {
    id: Type.String({ description: 'Keycloak user id' }),
    email: Type.Union([Type.String(), Type.Null()]),
    name: Type.Union([Type.String(), Type.Null()]),
    locale: Type.Union([Type.String(), Type.Null()]),
    avatar: Type.Union([Type.String(), Type.Null()], { description: 'Profile picture as a data URL' }),
    roles: Type.Array(Type.String(), { description: 'Realm roles' }),
    orgs: Type.Array(
      Type.Object({
        id: Type.String(),
        role: Type.String({ description: 'A built-in role\'s name (`admin`) or the id of a role the organization made' }),
        permissions: Type.Record(Type.String(), Type.String(), { description: 'What the role allows: action → scope (`own`, `group`, `org`)' }),
        groups: Type.Array(Type.String(), { description: 'Department group paths' }),
      }),
    ),
    activeOrgId: Type.Union([Type.String(), Type.Null()], { description: 'From `X-Org-Id`' }),
  },
  { description: 'The signed-in user' },
)

export const meRoutes: FastifyPluginAsyncTypebox = async (app) => {
  app.get(
    '/me',
    {
      schema: {
        tags: ['me'],
        summary: 'The signed-in user and their organizations',
        security: SECURITY,
        headers: OrgIdHeader,
        response: { 200: Me, ...problems(401, 403) },
      },
    },
    async (request) => {
      const user = request.user!
      const [row] = app.db ? await app.db.select({ avatar: schema.kcUser.avatar }).from(schema.kcUser).where(eq(schema.kcUser.id, user.id)) : []
      return {
        avatar: row?.avatar ?? null,
        id: user.id,
        email: user.email,
        name: user.name,
        locale: user.locale,
        roles: user.roles,
        orgs: Object.entries(user.orgs)
          .map(([id, m]) => ({ id, role: m.role, permissions: m.permissions, groups: m.groups }))
          .sort((a, b) => a.id.localeCompare(b.id)),
        activeOrgId: request.orgId,
      }
    },
  )

  app.put(
    '/me/avatar',
    {
      schema: {
        tags: ['me'],
        summary: 'Sets the signed-in user\'s profile picture',
        description: `A PNG, JPEG or WebP as a data URL, at most ${AVATAR_MAX} characters (the app shrinks pictures to a small square first). Everyone in the user's organizations sees it.`,
        security: SECURITY,
        body: Type.Object({ image: Type.String({ maxLength: AVATAR_MAX }) }),
        response: { 204: Type.Null(), ...problems(400, 401, 403, 503) },
      },
    },
    async (request, reply) => {
      if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
      if (!AVATAR.test(request.body.image)) throw new HttpProblem(400, 'bad_request', 'the picture must be a PNG, JPEG or WebP data URL')
      await app.db.update(schema.kcUser).set({ avatar: request.body.image }).where(eq(schema.kcUser.id, request.user!.id))
      return reply.code(204).send(null)
    },
  )

  app.delete(
    '/me/avatar',
    {
      schema: {
        tags: ['me'],
        summary: 'Removes the signed-in user\'s profile picture',
        security: SECURITY,
        response: { 204: Type.Null(), ...problems(401, 403, 503) },
      },
    },
    async (request, reply) => {
      if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
      await app.db.update(schema.kcUser).set({ avatar: null }).where(eq(schema.kcUser.id, request.user!.id))
      return reply.code(204).send(null)
    },
  )
}
