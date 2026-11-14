// Bearer authentication with Keycloak access tokens (SPEC §7.2).
//
// A route is protected when its schema declares `security: [{ keycloak: [] }]`,
// so the API docs and the enforcement cannot disagree. The token is verified
// against the realm's JWKS: signature, `iss`, `aud`, `exp`, `typ` and the
// client (`azp`). Org roles come from group paths in the `groups` claim:
//   /orgs/<orgId>/roles/<owner|admin|manager|member>   role in the org
//   /orgs/<orgId>/groups/<…>                           departments
// The active org is picked with the `X-Org-Id` header.

import type { FastifyInstance, FastifyRequest } from 'fastify'
import fp from 'fastify-plugin'
import { createRemoteJWKSet, errors as joseErrors, type JWTPayload, jwtVerify, type JWTVerifyGetKey } from 'jose'
import { Type } from 'typebox'
import type { Config } from '../config.ts'
import { HttpProblem } from './errors.ts'
import { defaultPermissions, isBuiltinRole, ORG_ROLES, type Permissions, type RoleKey } from './permissions.ts'

export const SECURITY = [{ keycloak: [] as string[] }]

export { ORG_ROLES }
export type OrgRole = RoleKey

export interface OrgMembership {
  /** A built-in role's name, or the id of a role the organization made. */
  role: OrgRole
  /** What the role allows: the built-in defaults here, an organization's own changes added by `rolesPlugin`. */
  permissions: Permissions
  /** Full paths of the department groups the user belongs to. */
  groups: string[]
}

export interface AuthUser {
  /** Keycloak user id (`sub`). */
  id: string
  email: string | null
  /** Keycloak has checked that the user owns the address. */
  emailVerified: boolean
  name: string | null
  locale: string | null
  /** Realm roles, e.g. `user`, `platform-admin`. */
  roles: string[]
  orgs: Record<string, OrgMembership>
  /** Client the token was issued to (`azp`). */
  client: string
  /** When the token expires, unix ms. */
  expiresAt: number
}

declare module 'fastify' {
  interface FastifyRequest {
    /** Set on routes with `security`; `null` elsewhere. */
    user: AuthUser | null
    /** From `X-Org-Id`, checked against the user's orgs. */
    orgId: string | null
  }
}

export const OrgIdHeader = Type.Object({
  'x-org-id': Type.Optional(Type.String({ description: 'Active organization; must be one of the user\'s orgs.' })),
})

/** Org memberships from the token's group paths. */
export function orgsFromGroups(groups: readonly string[]): Record<string, OrgMembership> {
  const orgs: Record<string, OrgMembership> = {}
  const rank = (r: OrgRole) => (isBuiltinRole(r) ? ORG_ROLES.indexOf(r) : ORG_ROLES.length)
  for (const path of groups) {
    const [, root, orgId, kind, ...rest] = path.split('/')
    if (root !== 'orgs' || !orgId || !kind) continue
    let org = orgs[orgId]
    if (!org) {
      org = { role: '', permissions: {}, groups: [] }
      orgs[orgId] = org
    }
    if (kind === 'roles' && rest.length === 1 && /^[A-Za-z0-9-]{1,64}$/.test(rest[0]!)) {
      const role = rest[0]!
      if (org.role === '' || rank(role) < rank(org.role)) org.role = role
    } else if (kind === 'groups' && rest.length > 0) {
      org.groups.push(path)
    }
  }
  for (const org of Object.values(orgs)) {
    // In the organization without a role group a person is a plain member.
    if (org.role === '') org.role = 'member'
    org.permissions = isBuiltinRole(org.role) ? defaultPermissions(org.role) : {}
    org.groups.sort()
  }
  return orgs
}

const str = (v: unknown) => (typeof v === 'string' && v !== '' ? v : null)

function userFromClaims(claims: JWTPayload): AuthUser {
  const groups = Array.isArray(claims.groups) ? claims.groups.filter((g): g is string => typeof g === 'string') : []
  const realmAccess = claims.realm_access as { roles?: unknown } | undefined
  const roles = Array.isArray(realmAccess?.roles) ? realmAccess.roles.filter((r): r is string => typeof r === 'string') : []
  return {
    id: claims.sub!,
    email: str(claims.email),
    emailVerified: claims.email_verified === true,
    name: str(claims.name),
    locale: str(claims.locale),
    roles,
    orgs: orgsFromGroups(groups),
    client: str(claims.azp) ?? '',
    expiresAt: (claims.exp ?? 0) * 1000,
  }
}

export interface AuthOptions {
  config: Config
  /** Signing keys; defaults to the realm's JWKS endpoint. Tests pass local keys. */
  keys?: JWTVerifyGetKey
}

const unauthorized = (detail: string, error = 'invalid_token') =>
  new HttpProblem(401, 'unauthorized', detail, {
    'www-authenticate': `Bearer realm="zeima", error="${error}", error_description="${detail.replace(/"/g, "'")}"`,
  })

function requiresAuth(request: FastifyRequest): boolean {
  const security = (request.routeOptions.schema as { security?: Record<string, unknown>[] } | undefined)?.security
  return security?.some((s) => 'keycloak' in s) ?? false
}

export const authPlugin = fp(
  async (app: FastifyInstance, { config, keys }: AuthOptions) => {
    const getKey =
      keys ?? createRemoteJWKSet(new URL(config.keycloak.jwksUrl), { timeoutDuration: 5_000, cooldownDuration: 30_000 })

    app.decorateRequest('user', null)
    app.decorateRequest('orgId', null)

    app.addHook('onRequest', async (request) => {
      if (!requiresAuth(request)) return
      const header = request.headers.authorization
      const token = header?.match(/^Bearer\s+(\S+)$/i)?.[1]
      if (!token) throw new HttpProblem(401, 'unauthorized', 'a bearer token is required', { 'www-authenticate': 'Bearer realm="zeima"' })

      let claims: JWTPayload
      try {
        ;({ payload: claims } = await jwtVerify(token, getKey, {
          issuer: config.keycloak.issuer,
          audience: config.audience,
          algorithms: ['RS256', 'RS384', 'RS512', 'PS256', 'ES256', 'ES384', 'EdDSA'],
          clockTolerance: 5,
          requiredClaims: ['sub', 'exp'],
        }))
      } catch (err) {
        if (err instanceof joseErrors.JWTExpired) throw unauthorized('the token has expired')
        if (err instanceof joseErrors.JOSEError) throw unauthorized('the token is not valid')
        throw err // JWKS unreachable etc.: a server problem
      }
      // Keycloak marks access tokens; ID and refresh tokens are not accepted.
      if (claims.typ !== 'Bearer') throw unauthorized('not an access token')
      const user = userFromClaims(claims)
      if (!config.allowedClients.includes(user.client)) {
        throw new HttpProblem(403, 'client_not_allowed', `tokens of client '${user.client}' are not accepted`)
      }
      request.user = user

      const orgId = request.headers['x-org-id']
      if (typeof orgId === 'string' && orgId !== '') {
        if (!user.orgs[orgId]) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
        request.orgId = orgId
      }
    })
  },
  { name: 'auth', dependencies: ['errors'] },
)
