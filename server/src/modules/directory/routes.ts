// A company directory (LDAP or Active Directory) people can sign in with, for self-hosted
// installations (Phase 7). Keycloak does it as user federation: a person signs in on the Keycloak
// page with their directory name and password, and their account is imported. These routes are how
// the people who run the installation (realm role `platform-admin`) set it up from the app instead of
// Keycloak's console. Read only: Zeima never writes to the directory.

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { type Static, Type } from 'typebox'
import type { LdapInput } from '../../keycloak/admin.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'

const ADMIN_ROLE = 'platform-admin'

const Vendor = Type.Union([Type.Literal('ad'), Type.Literal('openldap')], { description: '`ad` for Active Directory, `openldap` for OpenLDAP, FreeIPA, 389 and the like' })

const LdapView = Type.Object(
  {
    configured: Type.Boolean(),
    vendor: Type.Optional(Vendor),
    url: Type.Optional(Type.String()),
    startTls: Type.Optional(Type.Boolean()),
    bindDn: Type.Optional(Type.String()),
    usersDn: Type.Optional(Type.String()),
    userFilter: Type.Optional(Type.String()),
    enabled: Type.Boolean(),
  },
  { description: 'The directory people can sign in with; the password of the bind account is never returned' },
)

const LdapBody = Type.Object({
  vendor: Vendor,
  url: Type.String({ minLength: 1, maxLength: 300, description: '`ldap://host:389` or `ldaps://host:636`' }),
  startTls: Type.Optional(Type.Boolean({ default: false })),
  bindDn: Type.String({ minLength: 1, maxLength: 500, description: 'The account that reads the directory' }),
  bindPassword: Type.Optional(Type.String({ minLength: 1, maxLength: 500, description: 'Left out on a change, the stored password stays' })),
  usersDn: Type.String({ minLength: 1, maxLength: 500, description: 'Where the people are, `ou=people,dc=acme,dc=example`' }),
  userFilter: Type.Optional(Type.String({ maxLength: 1000, description: 'An LDAP filter that limits who may sign in, in parentheses' })),
  enabled: Type.Optional(Type.Boolean({ default: true })),
})

export function checkLdapUrl(raw: string): void {
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    throw new HttpProblem(400, 'bad_request', 'that is not an address')
  }
  if ((url.protocol !== 'ldap:' && url.protocol !== 'ldaps:') || !url.hostname || url.username || url.password || (url.pathname !== '' && url.pathname !== '/') || url.search) {
    throw new HttpProblem(400, 'bad_request', 'the address must look like ldaps://host:636')
  }
}

export function checkFilter(filter: string | undefined): void {
  if (!filter) return
  let depth = 0
  for (const ch of filter) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (depth < 0) break
  }
  if (!filter.startsWith('(') || !filter.endsWith(')') || depth !== 0) throw new HttpProblem(400, 'bad_request', 'the filter must be in parentheses, like (memberOf=cn=zeima,ou=groups,dc=acme,dc=example)')
}

/** Keycloak refused a user-federation call: the service account has no `manage-realm`, which the rest of Zeima does not need. */
async function withRole<T>(work: () => Promise<T>): Promise<T> {
  try {
    return await work()
  } catch (err) {
    if (err instanceof HttpProblem && err.code === 'keycloak_forbidden') {
      throw new HttpProblem(
        409,
        'directory_needs_role',
        'the API\'s service account may not change the realm\'s user federation. For the company directory, give it the realm-management role manage-realm in Keycloak (Clients → zeima-api-admin → Service account roles), and take the role away again when you are done (docs/keycloak.md).',
      )
    }
    throw err
  }
}

export const directoryRoutes: FastifyPluginAsyncTypebox = async (app) => {
  const kc = () => {
    if (!app.keycloakAdmin) throw new HttpProblem(503, 'unavailable', 'organization management is not set up')
    return app.keycloakAdmin
  }
  const requireAdmin = (roles: readonly string[]) => {
    if (!roles.includes(ADMIN_ROLE)) throw new HttpProblem(403, 'forbidden', `this needs the ${ADMIN_ROLE} role`)
  }
  const input = (body: Static<typeof LdapBody>): LdapInput => {
    checkLdapUrl(body.url)
    checkFilter(body.userFilter)
    return {
      vendor: body.vendor,
      url: body.url,
      startTls: body.startTls ?? false,
      bindDn: body.bindDn,
      ...(body.bindPassword !== undefined && { bindPassword: body.bindPassword }),
      usersDn: body.usersDn,
      ...(body.userFilter !== undefined && { userFilter: body.userFilter }),
      enabled: body.enabled ?? true,
    }
  }

  app.get(
    '/admin/ldap',
    { schema: { tags: ['directory'], summary: 'The directory people can sign in with', security: SECURITY, response: { 200: LdapView, ...problems(401, 403, 409, 502, 503) } } },
    async (request) => {
      requireAdmin(request.user!.roles)
      const ldap = await withRole(() => kc().getLdap())
      return ldap ? { configured: true, ...ldap } : { configured: false, enabled: false }
    },
  )

  app.put(
    '/admin/ldap',
    {
      schema: {
        tags: ['directory'],
        summary: 'Connects (or changes) the directory',
        description: 'Keycloak checks the connection first; nothing is saved when it fails. People are imported when they first sign in and then every day.',
        security: SECURITY,
        body: LdapBody,
        response: { 200: LdapView, ...problems(400, 401, 403, 409, 502, 503) },
      },
    },
    async (request) => {
      requireAdmin(request.user!.roles)
      const ldap = input(request.body)
      const keycloak = kc()
      return withRole(async () => {
        if (!(await keycloak.getLdap()) && !ldap.bindPassword) throw new HttpProblem(400, 'bad_request', 'the password of the bind account is required')
        await keycloak.testLdap(ldap)
        await keycloak.setLdap(ldap)
        request.log.info({ actor: request.user!.id, url: ldap.url }, 'directory set up')
        return { configured: true as const, ...(await keycloak.getLdap())! }
      })
    },
  )

  app.post(
    '/admin/ldap/test',
    {
      schema: {
        tags: ['directory'],
        summary: 'Checks that Keycloak can sign in to the directory with these settings',
        security: SECURITY,
        body: LdapBody,
        response: { 204: Type.Null(), ...problems(400, 401, 403, 409, 502, 503) },
      },
    },
    async (request, reply) => {
      requireAdmin(request.user!.roles)
      await withRole(() => kc().testLdap(input(request.body)))
      return reply.code(204).send(null)
    },
  )

  app.post(
    '/admin/ldap/sync',
    {
      schema: {
        tags: ['directory'],
        summary: 'Imports the directory\'s people now',
        security: SECURITY,
        response: { 200: Type.Object({ added: Type.Integer(), updated: Type.Integer(), removed: Type.Integer(), failed: Type.Integer() }), ...problems(401, 403, 404, 409, 502, 503) },
      },
    },
    async (request) => {
      requireAdmin(request.user!.roles)
      return withRole(() => kc().syncLdap())
    },
  )

  app.delete(
    '/admin/ldap',
    { schema: { tags: ['directory'], summary: 'Removes the directory; imported accounts stay but can no longer sign in with the directory\'s password', security: SECURITY, response: { 204: Type.Null(), ...problems(401, 403, 409, 502, 503) } } },
    async (request, reply) => {
      requireAdmin(request.user!.roles)
      await withRole(() => kc().deleteLdap())
      return reply.code(204).send(null)
    },
  )
}
