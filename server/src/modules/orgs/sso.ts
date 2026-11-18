// Single sign-on per organization (SPEC §7.2, Phase 7): an organization connects its own identity
// provider (Microsoft Entra ID, Google Workspace, any OIDC or SAML provider); people whose email
// address is in the organization's domains are sent there when they sign in and join the
// organization. Keycloak does the work (an identity provider linked to the organization,
// src/keycloak/admin.ts); these routes are how an owner or admin sets it up.

import { createHmac } from 'node:crypto'
import { lookup, resolveTxt } from 'node:dns/promises'
import { isIP } from 'node:net'
import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'
import type { Config } from '../../config.ts'
import { ssoAlias, type KeycloakAdmin, type SsoInput } from '../../keycloak/admin.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { authorize } from '../../plugins/authz.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { audit } from './service.ts'

const OrgParams = Type.Object({ orgId: Type.String({ format: 'uuid' }) })
const Protocol = Type.Union([Type.Literal('oidc'), Type.Literal('saml')])

const SsoView = Type.Object(
  {
    configured: Type.Boolean(),
    redirectUri: Type.String({ description: 'Give this to the identity provider as the address it redirects back to (OIDC) or posts the assertion to (SAML)' }),
    samlMetadataUrl: Type.String({ description: 'The service provider metadata of a SAML identity provider' }),
    domainChallenge: Type.Union([Type.Null(), Type.Object({ name: Type.String(), value: Type.String() })], {
      description: 'When domains must be proven: the TXT record to publish for each domain (`<name>.<domain>` with this value); `null` when not required',
    }),
    protocol: Type.Optional(Protocol),
    name: Type.Optional(Type.String()),
    sourceUrl: Type.Optional(Type.String()),
    clientId: Type.Optional(Type.String()),
    domains: Type.Array(Type.String()),
    enabled: Type.Boolean(),
  },
  { description: 'The organization\'s single sign-on; the client secret is never returned' },
)

const SsoBody = Type.Object({
  protocol: Protocol,
  name: Type.String({ minLength: 1, maxLength: 80, description: 'Shown to the people who sign in' }),
  sourceUrl: Type.String({ minLength: 1, maxLength: 500, description: 'OIDC: the issuer address; SAML: the metadata address' }),
  clientId: Type.Optional(Type.String({ minLength: 1, maxLength: 200 })),
  clientSecret: Type.Optional(Type.String({ minLength: 1, maxLength: 500, description: 'Left out on a change, the stored secret stays' })),
  domains: Type.Array(Type.String({ minLength: 1, maxLength: 253 }), { minItems: 1, maxItems: 10, description: 'Email domains whose people sign in here' }),
  enabled: Type.Optional(Type.Boolean({ default: true })),
})

const DOMAIN = /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/
export const CHALLENGE_NAME = '_zeima-challenge'

function privateAddress(ip: string): boolean {
  if (isIP(ip) === 4) {
    const [a = 0, b = 0] = ip.split('.').map(Number)
    return a === 10 || a === 127 || a === 0 || (a === 169 && b === 254) || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 100 && b >= 64 && b <= 127) || a >= 224
  }
  const v6 = ip.toLowerCase()
  if (v6.startsWith('::ffff:')) return privateAddress(v6.slice(7))
  return v6 === '::1' || v6 === '::' || v6.startsWith('fc') || v6.startsWith('fd') || v6.startsWith('fe8') || v6.startsWith('fe9') || v6.startsWith('fea') || v6.startsWith('feb')
}

/**
 * The address Keycloak is asked to read the provider's metadata from. Keycloak fetches it from its
 * own network, so an address that reaches inside it must be refused: https only, no login in the
 * address, no names or numbers on a private network (also after resolving the name).
 */
export type Resolver = (host: string) => Promise<{ address: string }[]>
const systemResolver: Resolver = (host) => lookup(host, { all: true })

export async function checkSourceUrl(raw: string, allowPrivate: boolean, resolver: Resolver = systemResolver): Promise<URL> {
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    throw new HttpProblem(400, 'bad_request', 'the address is not a web address')
  }
  if (url.username || url.password) throw new HttpProblem(400, 'bad_request', 'the address must not contain a user name or password')
  if (allowPrivate) {
    if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new HttpProblem(400, 'bad_request', 'the address must start with https://')
    return url
  }
  if (url.protocol !== 'https:') throw new HttpProblem(400, 'bad_request', 'the address must start with https://')
  const host = url.hostname.replace(/^\[|\]$/g, '').toLowerCase()
  const internalName = host === 'localhost' || host.endsWith('.localhost') || host.endsWith('.local') || host.endsWith('.internal') || !host.includes('.')
  if (isIP(host) || internalName) throw new HttpProblem(400, 'bad_request', 'the address must be a public host name')
  const found = await resolver(host).catch(() => [])
  if (found.length === 0) throw new HttpProblem(400, 'bad_request', 'the host of the address cannot be found')
  if (found.some((f) => privateAddress(f.address))) throw new HttpProblem(400, 'bad_request', 'the address must be a public host name')
  return url
}

export function normalizeDomains(domains: readonly string[]): string[] {
  const out = [...new Set(domains.map((d) => d.trim().toLowerCase().replace(/^@/, '')))]
  for (const d of out) {
    if (!DOMAIN.test(d)) throw new HttpProblem(400, 'bad_request', `"${d}" is not an email domain such as example.com`)
  }
  return out
}

export interface SsoRouteOptions {
  config: Config
  /** The TXT records of a name; the real DNS unless a test gives its own. */
  txt?: (name: string) => Promise<string[][]>
  /** Resolves a host name to its addresses; the real DNS unless a test gives its own. */
  resolver?: Resolver
}

export const ssoRoutes: FastifyPluginAsyncTypebox<SsoRouteOptions> = async (app, { config, txt = resolveTxt, resolver }) => {
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
  const actorIn = (request: { user: NonNullable<import('fastify').FastifyRequest['user']> }, orgId: string) => {
    if (!request.user.orgs[orgId]) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
    return { user: request.user, orgId }
  }

  // Single sign-on is an enterprise feature on the cloud.
  app.addHook('preHandler', async (request) => {
    if (request.user && app.licensing.enforced) await app.licensing.require(database(), request.user, 'sso')
  })

  /** Proves the organization owns a domain: the same value for every domain of the organization, from a secret only the API has. */
  const challengeValue = (orgId: string) =>
    `zeima-verification=${createHmac('sha256', config.keycloak.admin?.clientSecret ?? 'zeima-development').update(`sso-domain:${orgId}`).digest('hex').slice(0, 32)}`

  const owns = async (orgId: string, domain: string) => {
    const records = await txt(`${CHALLENGE_NAME}.${domain}`).catch(() => [] as string[][])
    return records.some((r) => r.join('') === challengeValue(orgId))
  }

  const view = async (orgId: string) => {
    const alias = ssoAlias(orgId)
    const sso = await keycloak().getSso(orgId)
    return {
      configured: sso !== null,
      redirectUri: `${config.keycloak.issuer}/broker/${alias}/endpoint`,
      samlMetadataUrl: `${config.keycloak.issuer}/broker/${alias}/endpoint/descriptor`,
      domainChallenge: config.sso.verifyDomains ? { name: CHALLENGE_NAME, value: challengeValue(orgId) } : null,
      ...(sso && { protocol: sso.protocol, name: sso.name, sourceUrl: sso.sourceUrl, ...(sso.clientId && { clientId: sso.clientId }) }),
      domains: sso?.domains ?? [],
      enabled: sso?.enabled ?? false,
    }
  }

  app.get(
    '/orgs/:orgId/sso',
    {
      schema: {
        tags: ['organizations'],
        summary: 'The single sign-on of the organization (those who manage its settings)',
        description: 'Also tells where the identity provider has to be pointed (`redirectUri`) before it is configured.',
        security: SECURITY,
        params: OrgParams,
        response: { 200: SsoView, ...problems(401, 403, 404, 502, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      authorize(actorIn({ user: request.user! }, orgId), 'org.settings', { orgId })
      return view(orgId)
    },
  )

  app.put(
    '/orgs/:orgId/sso',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Sets up or changes the single sign-on (those who manage the organization\'s settings)',
        description:
          'People whose email address is in `domains` are sent to the identity provider when they sign in, and join the organization. ' +
          'Where domains must be proven (`domainChallenge` is not null), each needs the TXT record. The provider\'s metadata is read from `sourceUrl` by Keycloak.',
        security: SECURITY,
        params: OrgParams,
        body: SsoBody,
        response: { 200: SsoView, ...problems(400, 401, 403, 404, 409, 502, 503) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'org.settings', { orgId })
      const body = request.body
      await checkSourceUrl(body.sourceUrl, config.sso.allowPrivateUrls, resolver)
      const domains = normalizeDomains(body.domains)
      if (body.protocol === 'oidc') {
        const existing = await keycloak().getSso(orgId)
        if (!body.clientId) throw new HttpProblem(400, 'bad_request', 'an OIDC provider needs a client id')
        if (!body.clientSecret && !(existing?.protocol === 'oidc')) throw new HttpProblem(400, 'bad_request', 'an OIDC provider needs a client secret')
      }
      if (config.sso.verifyDomains) {
        for (const domain of domains) {
          if (!(await owns(orgId, domain))) {
            throw new HttpProblem(400, 'domain_not_verified', `publish the TXT record ${CHALLENGE_NAME}.${domain} with the value ${challengeValue(orgId)}, then try again`)
          }
        }
      }
      const input: SsoInput = {
        protocol: body.protocol,
        name: body.name.trim(),
        sourceUrl: body.sourceUrl.trim(),
        ...(body.protocol === 'oidc' && { clientId: body.clientId, ...(body.clientSecret && { clientSecret: body.clientSecret }) }),
        domains,
        enabled: body.enabled ?? true,
      }
      await keycloak().setSso(orgId, input)
      await audit(database(), { orgId, actorId: actor.user.id, action: 'sso.set', target: ssoAlias(orgId), detail: { protocol: input.protocol, domains, enabled: input.enabled } })
      return view(orgId)
    },
  )

  app.delete(
    '/orgs/:orgId/sso',
    {
      schema: {
        tags: ['organizations'],
        summary: 'Removes the single sign-on; people sign in with their own accounts again (those who manage the organization\'s settings)',
        description: 'The people who joined through it stay members and keep their accounts.',
        security: SECURITY,
        params: OrgParams,
        response: { 204: Type.Null(), ...problems(401, 403, 404, 502, 503) },
      },
    },
    async (request, reply) => {
      const { orgId } = request.params
      const actor = actorIn({ user: request.user! }, orgId)
      authorize(actor, 'org.settings', { orgId })
      await keycloak().deleteSso(orgId)
      await audit(database(), { orgId, actorId: actor.user.id, action: 'sso.delete', target: ssoAlias(orgId) })
      return reply.code(204).send(null)
    },
  )
}
