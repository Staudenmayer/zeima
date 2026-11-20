// The Keycloak Admin REST API, as far as organization management needs it
// (SPEC §5.9, §7.2). Writes go to Keycloak first; the API's mirror tables
// follow. The API signs in as the service account `zeima-api-admin`.
//
// Layout in Keycloak, per organization (token `groups` claim, auth.ts):
//   Keycloak Organization <orgId>      members, invitations
//   /orgs/<orgId>/roles/<role>         one group per role; a user is in one
//   /orgs/<orgId>/groups/<…>           departments

import type { FastifyBaseLogger } from 'fastify'
import type { Config } from '../config.ts'
import { ORG_ROLES, type RoleKey } from '../plugins/permissions.ts'
import { HttpProblem } from '../plugins/errors.ts'

export interface KeycloakAdmin {
  /** Creates the organization and its role groups; returns the organization's id. */
  createOrganization(input: { name: string; alias: string }): Promise<string>
  deleteOrganization(orgId: string): Promise<void>
  /** Adds an existing user to the organization. */
  addMember(orgId: string, userId: string): Promise<void>
  /** Removes the user from the organization and from all its groups. */
  removeMember(orgId: string, userId: string): Promise<void>
  /** Puts the user in the group of `role` and out of the other role groups. */
  setRole(orgId: string, userId: string, role: RoleKey): Promise<void>
  /** Makes the Keycloak group of a role the organization defined (named by the role's id). */
  createRole(orgId: string, key: RoleKey): Promise<void>
  /** Removes the group of a role the organization defined. */
  deleteRole(orgId: string, key: RoleKey): Promise<void>
  /** Sends an invitation by email; the invitee joins the organization when they sign up. */
  invite(orgId: string, email: string): Promise<void>
  /** Disables (or re-enables) the user's account; their data stays. */
  setEnabled(userId: string, enabled: boolean): Promise<void>
  /** Creates a department (`['dev', 'backend']` is `/orgs/<id>/groups/dev/backend`); returns its group id. */
  createGroup(orgId: string, path: readonly string[]): Promise<string>
  /** The organization's members as Keycloak has them (for the nightly check of the mirror). */
  listMembers(orgId: string): Promise<KcUser[]>
  /** The departments under `/orgs/<orgId>/groups`, each with its members; paths are full. */
  listDepartments(orgId: string): Promise<KcDepartment[]>
  /** The people in each role group of the organization. */
  listRoleMembers(orgId: string): Promise<Record<RoleKey, string[]>>
  deleteGroup(groupId: string): Promise<void>
  /** Renames a department (its subgroups follow). */
  renameGroup(groupId: string, name: string): Promise<void>
  addToGroup(userId: string, groupId: string): Promise<void>
  removeFromGroup(userId: string, groupId: string): Promise<void>
  /** The identity provider (single sign-on) the organization has, if any. */
  getSso(orgId: string): Promise<KcSso | null>
  /** Has Keycloak parse a provider's metadata document (OIDC discovery JSON or SAML XML) into provider settings. Keycloak fetches nothing. */
  parseSsoMetadata(protocol: SsoProtocol, document: string): Promise<Record<string, string>>
  /** Sets up (or changes) the organization's identity provider and the email domains that sign in through it; `imported` is what `parseSsoMetadata` returned. */
  setSso(orgId: string, input: SsoInput, imported: Record<string, string>): Promise<void>
  /** Removes the identity provider and the organization's domains. */
  deleteSso(orgId: string): Promise<void>
  /** Ends every session of the user (their tokens stop working at the next refresh). */
  logoutUser(userId: string): Promise<void>
  /** The account with this email address, if there is one. */
  findUserByEmail(email: string): Promise<KcUser | null>
  /** Creates an account for a person an identity provider's directory knows; returns its id. */
  createUser(input: { email: string; firstName: string; lastName: string; enabled: boolean; emailVerified: boolean }): Promise<string>
  /** Changes a person's name. */
  setUserName(userId: string, firstName: string, lastName: string): Promise<void>
  /** The directory (LDAP / Active Directory) people can sign in with, if one is set up. */
  getLdap(): Promise<KcLdap | null>
  /** Sets up (or changes) the directory. */
  setLdap(input: LdapInput): Promise<void>
  /** Removes the directory; the accounts that were imported from it stay. */
  deleteLdap(): Promise<void>
  /** Checks that Keycloak can sign in to the directory with these settings. */
  testLdap(input: LdapInput): Promise<void>
  /** Imports the directory's people now. */
  syncLdap(): Promise<{ added: number; updated: number; removed: number; failed: number }>
}

export type LdapVendor = 'ad' | 'openldap'

export interface LdapInput {
  vendor: LdapVendor
  /** `ldap://host:389` or `ldaps://host:636`. */
  url: string
  startTls: boolean
  bindDn: string
  /** Left out on a change it stays as it was. */
  bindPassword?: string
  /** Where the people are, `ou=people,dc=acme,dc=example`. */
  usersDn: string
  /** An LDAP filter that limits who may sign in, `(memberOf=cn=zeima,…)`. */
  userFilter?: string
  enabled: boolean
}

export interface KcLdap {
  vendor: LdapVendor
  url: string
  startTls: boolean
  bindDn: string
  usersDn: string
  userFilter: string
  enabled: boolean
}

export type SsoProtocol = 'oidc' | 'saml'

export interface SsoInput {
  protocol: SsoProtocol
  /** Shown to the people who sign in. */
  name: string
  /** OIDC: the issuer address (`…/.well-known/openid-configuration` is read from it); SAML: the metadata address. */
  sourceUrl: string
  /** OIDC only. */
  clientId?: string
  /** OIDC only; left out on a change it stays as it was. */
  clientSecret?: string
  /** Email domains whose people are sent to this identity provider when they sign in. */
  domains: string[]
  enabled: boolean
}

export interface KcSso {
  protocol: SsoProtocol
  name: string
  enabled: boolean
  sourceUrl: string
  clientId: string | null
  domains: string[]
}

const LDAP_TYPE = 'org.keycloak.storage.UserStorageProvider'
const LDAP_NAME = 'zeima-ldap'

/** The Keycloak alias of an organization's identity provider; it is part of the address the provider redirects to. */
export const ssoAlias = (orgId: string) => `sso-${orgId}`

/** The imported settings that name an address; the provider is contacted at each, so each must be public. */
export const SSO_ADDRESS_KEYS = [
  'authorizationUrl', 'tokenUrl', 'userInfoUrl', 'jwksUrl', 'logoutUrl', 'tokenIntrospectionUrl', 'metadataDescriptorUrl',
  'singleSignOnServiceUrl', 'singleLogoutServiceUrl', 'artifactResolutionServiceUrl',
] as const

/**
 * The settings of a parsed OIDC or SAML document that are kept: its addresses, issuer, entity ids and
 * certificates, and the SAML logout binding. Everything else in the parser's answer is dropped, so a
 * document cannot set other provider options (the ones that matter are set below). The address keys come
 * from the one list above, so what is kept and what is checked cannot drift apart.
 */
export const IMPORTED_SSO_KEYS: ReadonlySet<string> = new Set<string>([
  ...SSO_ADDRESS_KEYS,
  'issuer', 'entityId', 'idpEntityId', 'signingCertificate', 'encryptionPublicKey', 'postBindingLogout',
])

/** Settings a document may only turn on: 'true' is kept, anything else is dropped, so it cannot lower security. */
const RATCHET_SSO_KEYS: readonly string[] = ['wantAuthnRequestsSigned']

const pickImported = (imported: Record<string, string>): Record<string, string> =>
  Object.fromEntries(
    Object.entries(imported).filter(([k, v]) => IMPORTED_SSO_KEYS.has(k) || (RATCHET_SSO_KEYS.includes(k) && v === 'true')),
  )

export interface KcUser {
  id: string
  email: string | null
  name: string | null
  enabled: boolean
}

export interface KcDepartment {
  id: string
  /** Full path, `/orgs/<orgId>/groups/dev/backend`. */
  path: string
  memberIds: string[]
}

interface IdpRep {
  alias: string
  displayName?: string
  providerId: string
  enabled?: boolean
  config?: Record<string, string>
}

interface GroupRep {
  id: string
  name: string
  path: string
  subGroups?: GroupRep[]
}

/** Keycloak over HTTP with a client-credentials token. */
export class HttpKeycloakAdmin implements KeycloakAdmin {
  private token: { value: string; expiresAt: number } | null = null

  private readonly admin: NonNullable<Config['keycloak']['admin']>
  private readonly fetchImpl: typeof fetch

  private readonly log: Pick<FastifyBaseLogger, 'warn'>

  constructor(admin: NonNullable<Config['keycloak']['admin']>, fetchImpl: typeof fetch = fetch, log: Pick<FastifyBaseLogger, 'warn'> = console) {
    this.admin = admin
    this.fetchImpl = fetchImpl
    this.log = log
  }

  /** The upstream detail (host, port, Keycloak's own text) goes to the log, never into a response. */
  private unreachable(err: unknown): HttpProblem {
    this.log.warn({ err }, 'cannot reach Keycloak')
    return new HttpProblem(502, 'keycloak_unreachable', 'cannot reach the identity provider')
  }

  private async accessToken(): Promise<string> {
    const now = Date.now()
    if (this.token && this.token.expiresAt - 30_000 > now) return this.token.value
    const res = await this.fetchImpl(this.admin.tokenUrl, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        grant_type: 'client_credentials',
        client_id: this.admin.clientId,
        client_secret: this.admin.clientSecret,
      }),
      signal: AbortSignal.timeout(10_000),
    }).catch((err: unknown) => {
      throw this.unreachable(err)
    })
    if (!res.ok) throw new HttpProblem(502, 'keycloak_auth_failed', `Keycloak refused the API's admin client (${res.status})`)
    const body = (await res.json()) as { access_token: string; expires_in: number }
    this.token = { value: body.access_token, expiresAt: now + body.expires_in * 1000 }
    return body.access_token
  }

  /** One call; Keycloak errors become problems (409 stays 409). */
  private async call(method: string, path: string, init: { json?: unknown; form?: Record<string, string>; multipart?: FormData } = {}): Promise<Response> {
    const headers: Record<string, string> = { authorization: `Bearer ${await this.accessToken()}` }
    let body: string | URLSearchParams | FormData | undefined
    if (init.multipart) {
      body = init.multipart
    } else if (init.json !== undefined) {
      headers['content-type'] = 'application/json'
      body = JSON.stringify(init.json)
    } else if (init.form) {
      headers['content-type'] = 'application/x-www-form-urlencoded'
      body = new URLSearchParams(init.form)
    }
    const res = await this.fetchImpl(`${this.admin.apiUrl}${path}`, { method, headers, body, signal: AbortSignal.timeout(15_000) }).catch(
      (err: unknown) => {
        throw this.unreachable(err)
      },
    )
    if (res.ok) return res
    const detail = await res.text().catch(() => '')
    if (res.status === 403) throw new HttpProblem(502, 'keycloak_forbidden', `Keycloak refused the API's service account for ${method} ${path}: it lacks a role (docs/keycloak.md, "Service account roles")`)
    if (res.status === 404) throw new HttpProblem(404, 'not_found', 'not found in Keycloak')
    this.log.warn({ method, path, status: res.status, detail: keycloakMessage(detail) }, 'Keycloak refused a request')
    if (res.status === 409) throw new HttpProblem(409, 'conflict', 'already exists')
    if (res.status === 400) throw new HttpProblem(400, 'bad_request', 'the identity provider rejected the request')
    throw new HttpProblem(502, 'keycloak_error', `the identity provider answered ${res.status}`)
  }

  /** The id from a `201 Location: …/<id>` response. */
  private createdId(res: Response): string {
    const id = res.headers.get('location')?.split('/').pop()
    if (!id) throw new HttpProblem(502, 'keycloak_error', 'Keycloak did not return the id of the new object')
    return id
  }

  private async groupId(path: readonly string[], create: boolean): Promise<string | null> {
    let parent: string | null = null
    for (const name of path) {
      const list = (await (parent === null
        ? this.call('GET', `/groups?search=${encodeURIComponent(name)}&exact=true&briefRepresentation=true`)
        : this.call('GET', `/groups/${parent}/children?briefRepresentation=true&first=0&max=1000`)
      ).then((r) => r.json())) as GroupRep[]
      const found = list.find((g) => g.name === name)
      if (found) {
        parent = found.id
        continue
      }
      if (!create) return null
      const res: Response = await this.call('POST', parent === null ? '/groups' : `/groups/${parent}/children`, { json: { name } })
      parent = this.createdId(res)
    }
    return parent
  }

  async createOrganization({ name, alias }: { name: string; alias: string }): Promise<string> {
    const res = await this.call('POST', '/organizations', { json: { name, alias, enabled: true, domains: [] } })
    const orgId = this.createdId(res)
    for (const role of ORG_ROLES) await this.groupId(['orgs', orgId, 'roles', role], true)
    await this.groupId(['orgs', orgId, 'groups'], true)
    return orgId
  }

  async deleteOrganization(orgId: string): Promise<void> {
    await this.call('DELETE', `/organizations/${orgId}`)
    const id = await this.groupId(['orgs', orgId], false)
    if (id) await this.call('DELETE', `/groups/${id}`)
  }

  async addMember(orgId: string, userId: string): Promise<void> {
    // The body is the bare user id as a JSON string.
    await this.call('POST', `/organizations/${orgId}/members`, { json: userId }).catch((err: unknown) => {
      if (err instanceof HttpProblem && err.status === 409) return // already a member
      throw err
    })
  }

  async removeMember(orgId: string, userId: string): Promise<void> {
    const root = await this.groupId(['orgs', orgId], false)
    if (root) await this.removeFromTree(userId, root)
    await this.call('DELETE', `/organizations/${orgId}/members/${userId}`).catch((err: unknown) => {
      if (err instanceof HttpProblem && err.status === 404) return
      throw err
    })
  }

  /** Takes the user out of every group below `groupId` (and of `groupId`). */
  private async removeFromTree(userId: string, groupId: string): Promise<void> {
    const memberships = (await this.call('GET', `/users/${userId}/groups?briefRepresentation=true&max=1000`).then((r) => r.json())) as GroupRep[]
    const root = (await this.call('GET', `/groups/${groupId}`).then((r) => r.json())) as GroupRep
    for (const g of memberships) {
      if (g.path === root.path || g.path.startsWith(`${root.path}/`)) await this.call('DELETE', `/users/${userId}/groups/${g.id}`)
    }
  }

  /** The role groups of the organization by role key. */
  private async roleGroups(orgId: string): Promise<{ key: string; id: string }[]> {
    const parent = await this.groupId(['orgs', orgId, 'roles'], false)
    if (!parent) return []
    return (await this.all<GroupRep>(`/groups/${parent}/children?briefRepresentation=true`)).map((g) => ({ key: g.name, id: g.id }))
  }

  async setRole(orgId: string, userId: string, role: RoleKey): Promise<void> {
    const target = await this.groupId(['orgs', orgId, 'roles', role], true)
    for (const other of await this.roleGroups(orgId)) {
      if (other.key === role) continue
      await this.call('DELETE', `/users/${userId}/groups/${other.id}`).catch((err: unknown) => {
        if (err instanceof HttpProblem && err.status === 404) return
        throw err
      })
    }
    await this.call('PUT', `/users/${userId}/groups/${target}`)
  }

  async createRole(orgId: string, key: RoleKey): Promise<void> {
    await this.groupId(['orgs', orgId, 'roles', key], true)
  }

  async deleteRole(orgId: string, key: RoleKey): Promise<void> {
    const id = await this.groupId(['orgs', orgId, 'roles', key], false)
    if (id) await this.call('DELETE', `/groups/${id}`)
  }

  async invite(orgId: string, email: string): Promise<void> {
    await this.call('POST', `/organizations/${orgId}/members/invite-user`, { form: { email } })
  }

  async setEnabled(userId: string, enabled: boolean): Promise<void> {
    await this.call('PUT', `/users/${userId}`, { json: { enabled } })
  }

  async createGroup(orgId: string, path: readonly string[]): Promise<string> {
    const id = await this.groupId(['orgs', orgId, 'groups', ...path], true)
    return id!
  }

  /** Every page of a Keycloak list. */
  private async all<T>(path: string): Promise<T[]> {
    const out: T[] = []
    const sep = path.includes('?') ? '&' : '?'
    for (let first = 0; ; first += 100) {
      const page = (await this.call('GET', `${path}${sep}first=${first}&max=100`).then((r) => r.json())) as T[]
      out.push(...page)
      if (page.length < 100) return out
    }
  }

  async listMembers(orgId: string): Promise<KcUser[]> {
    const users = await this.all<{ id: string; email?: string; firstName?: string; lastName?: string; enabled?: boolean }>(`/organizations/${orgId}/members`)
    return users.map((u) => ({
      id: u.id,
      email: u.email ?? null,
      name: [u.firstName, u.lastName].filter(Boolean).join(' ') || null,
      enabled: u.enabled !== false,
    }))
  }

  async listDepartments(orgId: string): Promise<KcDepartment[]> {
    const root = await this.groupId(['orgs', orgId, 'groups'], false)
    if (!root) return []
    const out: KcDepartment[] = []
    const walk = async (id: string) => {
      for (const child of await this.all<GroupRep>(`/groups/${id}/children?briefRepresentation=true`)) {
        const members = await this.all<{ id: string }>(`/groups/${child.id}/members`)
        out.push({ id: child.id, path: child.path, memberIds: members.map((m) => m.id) })
        await walk(child.id)
      }
    }
    await walk(root)
    return out
  }

  async listRoleMembers(orgId: string): Promise<Record<RoleKey, string[]>> {
    const out: Record<RoleKey, string[]> = { owner: [], admin: [], manager: [], member: [] }
    for (const { key, id } of await this.roleGroups(orgId)) out[key] = (await this.all<{ id: string }>(`/groups/${id}/members`)).map((m) => m.id)
    return out
  }

  async deleteGroup(groupId: string): Promise<void> {
    await this.call('DELETE', `/groups/${groupId}`)
  }

  async renameGroup(groupId: string, name: string): Promise<void> {
    await this.call('PUT', `/groups/${groupId}`, { json: { name } })
  }

  async addToGroup(userId: string, groupId: string): Promise<void> {
    await this.call('PUT', `/users/${userId}/groups/${groupId}`)
  }

  async removeFromGroup(userId: string, groupId: string): Promise<void> {
    await this.call('DELETE', `/users/${userId}/groups/${groupId}`)
  }

  private async idp(alias: string): Promise<IdpRep | null> {
    const res = await this.call('GET', `/identity-provider/instances/${encodeURIComponent(alias)}`).catch((err: unknown) => {
      if (err instanceof HttpProblem && err.status === 404) return null
      throw err
    })
    return res ? ((await res.json()) as IdpRep) : null
  }

  /**
   * The organization's domains, each tied to the provider `alias`: people with such an address are sent to
   * the provider as soon as they give their address on the sign-in page, and join the organization.
   */
  private async setDomains(orgId: string, domains: readonly string[], alias: string): Promise<void> {
    const org = (await this.call('GET', `/organizations/${orgId}`).then((r) => r.json())) as Record<string, unknown>
    await this.call('PUT', `/organizations/${orgId}`, {
      json: { ...org, domains: domains.map((name) => ({ name, verified: false, autoRedirect: true, identityProviderAlias: alias })) },
    })
  }

  async getSso(orgId: string): Promise<KcSso | null> {
    const alias = ssoAlias(orgId)
    const linked = (await this.call('GET', `/organizations/${orgId}/identity-providers`).then((r) => r.json())) as IdpRep[]
    const idp = linked.find((p) => p.alias === alias)
    if (!idp) return null
    const config = idp.config ?? {}
    const org = (await this.call('GET', `/organizations/${orgId}`).then((r) => r.json())) as { domains?: { name: string; identityProviderAlias?: string }[] }
    return {
      protocol: idp.providerId === 'saml' ? 'saml' : 'oidc',
      name: idp.displayName ?? '',
      enabled: idp.enabled !== false,
      sourceUrl: config['zeima.sourceUrl'] ?? '',
      clientId: config['clientId'] ?? null,
      domains: (org.domains ?? []).filter((d) => d.identityProviderAlias === alias).map((d) => d.name),
    }
  }

  async parseSsoMetadata(protocol: SsoProtocol, document: string): Promise<Record<string, string>> {
    // The document goes up as a file: Keycloak parses it and does not go to any address itself.
    const form = new FormData()
    form.set('providerId', protocol)
    form.set('file', new Blob([document], { type: protocol === 'oidc' ? 'application/json' : 'application/xml' }), protocol === 'oidc' ? 'metadata.json' : 'metadata.xml')
    return (await this.call('POST', '/identity-provider/import-config', { multipart: form }).then((r) => r.json())) as Record<string, string>
  }

  async setSso(orgId: string, input: SsoInput, imported: Record<string, string>): Promise<void> {
    const alias = ssoAlias(orgId)
    const existing = await this.idp(alias)
    const config: Record<string, string> = {
      ...pickImported(imported),
      syncMode: 'IMPORT',
      'zeima.sourceUrl': input.sourceUrl,
    }
    if (input.protocol === 'oidc') {
      Object.assign(config, {
        clientId: input.clientId ?? '',
        // Keycloak shows a secret as ********** and keeps the stored one when it gets that back.
        clientSecret: input.clientSecret ?? (existing ? '**********' : ''),
        clientAuthMethod: 'client_secret_post',
        defaultScope: 'openid email profile',
        validateSignature: 'true',
        useJwksUrl: 'true',
        // The provider can tell Keycloak that a person signed out or was disabled there.
        backchannelSupported: 'true',
      })
    } else {
      Object.assign(config, {
        nameIDPolicyFormat: 'urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress',
        postBindingResponse: 'true',
        postBindingAuthnRequest: 'true',
        wantAssertionsSigned: 'true',
        validateSignature: 'true',
      })
    }
    // The address is not trusted to be the person's: Keycloak still confirms an address that matches
    // an existing account. The provider is not listed on the sign-in page; people reach it by their address.
    const rep = { alias, displayName: input.name, providerId: input.protocol, enabled: input.enabled, trustEmail: false, hideOnLogin: true, config }
    if (existing) {
      await this.call('PUT', `/identity-provider/instances/${encodeURIComponent(alias)}`, { json: { ...existing, ...rep } })
    } else {
      await this.call('POST', '/identity-provider/instances', { json: rep })
    }
    await this.call('POST', `/organizations/${orgId}/identity-providers`, { json: alias }).catch((err: unknown) => {
      if (err instanceof HttpProblem && err.status === 409) return // already linked
      throw err
    })
    // The domains point at the provider, so they come last.
    await this.setDomains(orgId, input.domains, alias)
  }

  async deleteSso(orgId: string): Promise<void> {
    const alias = ssoAlias(orgId)
    // The domains point at the provider, so they go first.
    await this.setDomains(orgId, [], alias)
    await this.call('DELETE', `/identity-provider/instances/${encodeURIComponent(alias)}`).catch((err: unknown) => {
      if (err instanceof HttpProblem && err.status === 404) return
      throw err
    })
  }

  async logoutUser(userId: string): Promise<void> {
    await this.call('POST', `/users/${userId}/logout`)
  }

  async findUserByEmail(email: string): Promise<KcUser | null> {
    const users = (await this.call('GET', `/users?email=${encodeURIComponent(email)}&exact=true&max=2`).then((r) => r.json())) as {
      id: string
      email?: string
      firstName?: string
      lastName?: string
      enabled?: boolean
    }[]
    const u = users.find((x) => x.email?.toLowerCase() === email.toLowerCase())
    return u
      ? { id: u.id, email: u.email ?? null, name: [u.firstName, u.lastName].filter(Boolean).join(' ') || null, enabled: u.enabled !== false }
      : null
  }

  async createUser(input: { email: string; firstName: string; lastName: string; enabled: boolean; emailVerified: boolean }): Promise<string> {
    // The directory vouches for the address: no verification mail.
    const res = await this.call('POST', '/users', {
      json: { username: input.email, email: input.email, firstName: input.firstName, lastName: input.lastName, enabled: input.enabled, emailVerified: input.emailVerified },
    })
    return this.createdId(res)
  }

  async setUserName(userId: string, firstName: string, lastName: string): Promise<void> {
    const user = (await this.call('GET', `/users/${userId}`).then((r) => r.json())) as Record<string, unknown>
    await this.call('PUT', `/users/${userId}`, { json: { ...user, firstName, lastName } })
  }

  private async ldapComponent(): Promise<{ id: string; config: Record<string, string[]>; parentId: string } | null> {
    const list = (await this.call('GET', `/components?type=${encodeURIComponent(LDAP_TYPE)}&name=${LDAP_NAME}`).then((r) => r.json())) as {
      id: string
      providerId: string
      name: string
      parentId: string
      config: Record<string, string[]>
    }[]
    return list.find((c) => c.providerId === 'ldap' && c.name === LDAP_NAME) ?? null
  }

  private ldapConfig(input: LdapInput, kept: string | undefined): Record<string, string[]> {
    const ad = input.vendor === 'ad'
    const c: Record<string, string> = {
      enabled: String(input.enabled),
      vendor: ad ? 'ad' : 'other',
      connectionUrl: input.url,
      startTls: String(input.startTls),
      authType: 'simple',
      bindDn: input.bindDn,
      bindCredential: input.bindPassword ?? kept ?? '',
      usersDn: input.usersDn,
      usernameLDAPAttribute: ad ? 'sAMAccountName' : 'uid',
      rdnLDAPAttribute: ad ? 'cn' : 'uid',
      uuidLDAPAttribute: ad ? 'objectGUID' : 'entryUUID',
      userObjectClasses: ad ? 'person, organizationalPerson, user' : 'inetOrgPerson, organizationalPerson',
      customUserSearchFilter: input.userFilter ?? '',
      searchScope: '2',
      pagination: 'true',
      connectionPooling: 'true',
      // Read only: Zeima never writes to the directory. The people are imported on their first sign-in and every day.
      editMode: 'READ_ONLY',
      importEnabled: 'true',
      syncRegistrations: 'false',
      // The directory vouches for the address, so invitations to it work.
      trustEmail: 'true',
      fullSyncPeriod: '86400',
      changedSyncPeriod: '-1',
      cachePolicy: 'DEFAULT',
      batchSizeForSync: '1000',
      validatePasswordPolicy: 'false',
      usePasswordModifyExtendedOp: 'false',
    }
    return Object.fromEntries(Object.entries(c).map(([k, v]) => [k, [v]]))
  }

  async getLdap(): Promise<KcLdap | null> {
    const c = await this.ldapComponent()
    if (!c) return null
    const one = (k: string) => c.config[k]?.[0] ?? ''
    return {
      vendor: one('vendor') === 'ad' ? 'ad' : 'openldap',
      url: one('connectionUrl'),
      startTls: one('startTls') === 'true',
      bindDn: one('bindDn'),
      usersDn: one('usersDn'),
      userFilter: one('customUserSearchFilter'),
      enabled: one('enabled') !== 'false',
    }
  }

  async setLdap(input: LdapInput): Promise<void> {
    const existing = await this.ldapComponent()
    const kept = existing ? '**********' : undefined
    const config = this.ldapConfig(input, kept)
    if (existing) {
      await this.call('PUT', `/components/${existing.id}`, { json: { id: existing.id, name: LDAP_NAME, providerId: 'ldap', providerType: LDAP_TYPE, parentId: existing.parentId, config } })
    } else {
      const realm = (await this.call('GET', '').then((r) => r.json())) as { id: string }
      await this.call('POST', '/components', { json: { name: LDAP_NAME, providerId: 'ldap', providerType: LDAP_TYPE, parentId: realm.id, config } })
    }
  }

  async deleteLdap(): Promise<void> {
    const existing = await this.ldapComponent()
    if (existing) await this.call('DELETE', `/components/${existing.id}`)
  }

  async testLdap(input: LdapInput): Promise<void> {
    const existing = await this.ldapComponent()
    // Keycloak takes the stored password when it gets ********** and the id of the component.
    await this.call('POST', '/testLDAPConnection', {
      json: {
        action: 'testAuthentication',
        connectionUrl: input.url,
        bindDn: input.bindDn,
        bindCredential: input.bindPassword ?? (existing ? '**********' : ''),
        useTruststoreSpi: 'ldapsOnly',
        connectionTimeout: '10000',
        authType: 'simple',
        startTls: String(input.startTls),
        ...(existing && { componentId: existing.id }),
      },
    })
  }

  async syncLdap() {
    const existing = await this.ldapComponent()
    if (!existing) throw new HttpProblem(404, 'not_found', 'no directory is set up')
    const r = (await this.call('POST', `/user-storage/${existing.id}/sync?action=triggerFullSync`).then((x) => x.json())) as {
      added?: number
      updated?: number
      removed?: number
      failed?: number
    }
    return { added: r.added ?? 0, updated: r.updated ?? 0, removed: r.removed ?? 0, failed: r.failed ?? 0 }
  }
}

function keycloakMessage(body: string): string {
  try {
    const json = JSON.parse(body) as { errorMessage?: string; error_description?: string; error?: string }
    return json.errorMessage ?? json.error_description ?? json.error ?? ''
  } catch {
    return ''
  }
}
