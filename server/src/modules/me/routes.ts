// The signed-in user as the API sees them: identity and org memberships.

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'
import { OrgIdHeader, SECURITY } from '../../plugins/auth.ts'
import { problems } from '../../plugins/errors.ts'

const Me = Type.Object(
  {
    id: Type.String({ description: 'Keycloak user id' }),
    email: Type.Union([Type.String(), Type.Null()]),
    name: Type.Union([Type.String(), Type.Null()]),
    locale: Type.Union([Type.String(), Type.Null()]),
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
      return {
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
}
