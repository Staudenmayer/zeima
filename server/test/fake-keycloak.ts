// A Keycloak admin client that only records what it was asked to do.

import { randomUUID } from 'node:crypto'
import { HttpProblem } from '../src/plugins/errors.ts'
import type { KcDepartment, KcLdap, KcSso, KcUser, KeycloakAdmin, LdapInput, SsoInput } from '../src/keycloak/admin.ts'
import type { RoleKey } from '../src/plugins/permissions.ts'

export class FakeKeycloak implements KeycloakAdmin {
  calls: string[] = []
  /** Set to make the next call of that name fail. */
  failNext: string | null = null

  private record(name: string, ...args: unknown[]) {
    if (this.failNext === name) {
      this.failNext = null
      throw new Error(`${name} failed`)
    }
    this.calls.push([name, ...args].join(' '))
  }

  /** What the next `list…` calls report: Keycloak's view of an organization. */
  members: KcUser[] = []
  departments: KcDepartment[] = []
  roles: Record<RoleKey, string[]> = { owner: [], admin: [], manager: [], member: [] }

  async listMembers(_orgId: string) {
    return this.members
  }
  async listDepartments(_orgId: string) {
    return this.departments
  }
  async listRoleMembers(_orgId: string) {
    return this.roles
  }

  async createOrganization(input: { name: string; alias: string }) {
    this.record('createOrganization', input.name)
    return randomUUID()
  }
  async deleteOrganization(orgId: string) {
    this.record('deleteOrganization', orgId)
  }
  async addMember(orgId: string, userId: string) {
    this.record('addMember', orgId, userId)
  }
  async removeMember(orgId: string, userId: string) {
    this.record('removeMember', orgId, userId)
  }
  async setRole(orgId: string, userId: string, role: string) {
    this.record('setRole', orgId, userId, role)
  }
  async createRole(orgId: string, key: string) {
    this.record('createRole', orgId, key)
  }
  async deleteRole(orgId: string, key: string) {
    this.record('deleteRole', orgId, key)
  }
  async invite(orgId: string, email: string) {
    this.record('invite', orgId, email)
  }
  async setEnabled(userId: string, enabled: boolean) {
    this.record('setEnabled', userId, enabled)
  }
  async createGroup(orgId: string, path: readonly string[]) {
    this.record('createGroup', orgId, path.join('/'))
    return randomUUID()
  }
  async deleteGroup(groupId: string) {
    this.record('deleteGroup', groupId)
  }
  async renameGroup(groupId: string, name: string) {
    this.record('renameGroup', groupId, name)
  }
  async addToGroup(userId: string, groupId: string) {
    this.record('addToGroup', userId, groupId)
  }
  async removeFromGroup(userId: string, groupId: string) {
    this.record('removeFromGroup', userId, groupId)
  }

  /** The identity provider of each organization, as the last `setSso` left it. */
  sso = new Map<string, KcSso & { clientSecret?: string }>()
  async getSso(orgId: string) {
    const kept = this.sso.get(orgId)
    if (!kept) return null
    const { clientSecret: _secret, ...sso } = kept
    return sso
  }
  /** What `parseSsoMetadata` hands back; a test sets it to try a provider that names other addresses. */
  metadata: Record<string, string> = { issuer: 'https://login.acme.example', tokenUrl: 'https://login.acme.example/token', authorizationUrl: 'https://login.acme.example/auth', jwksUrl: 'https://login.acme.example/keys' }
  async parseSsoMetadata(protocol: string, _document: string) {
    this.record('parseSsoMetadata', protocol)
    return this.metadata
  }
  async setSso(orgId: string, input: SsoInput, _imported: Record<string, string>) {
    this.record('setSso', orgId, input.protocol, input.domains.join(','))
    const before = this.sso.get(orgId)
    this.sso.set(orgId, {
      protocol: input.protocol,
      name: input.name,
      enabled: input.enabled,
      sourceUrl: input.sourceUrl,
      clientId: input.clientId ?? null,
      domains: input.domains,
      clientSecret: input.clientSecret ?? before?.clientSecret,
    })
  }
  async deleteSso(orgId: string) {
    this.record('deleteSso', orgId)
    this.sso.delete(orgId)
  }

  /** The accounts that exist in Keycloak, by id (for SCIM). */
  users = new Map<string, KcUser>()
  async logoutUser(userId: string) {
    this.record('logoutUser', userId)
  }
  async findUserByEmail(email: string) {
    return [...this.users.values()].find((u) => u.email?.toLowerCase() === email.toLowerCase()) ?? null
  }
  async createUser(input: { email: string; firstName: string; lastName: string; enabled: boolean; emailVerified: boolean }) {
    this.record('createUser', input.email, input.emailVerified ? 'verified' : 'unverified')
    const id = randomUUID()
    this.users.set(id, { id, email: input.email, name: `${input.firstName} ${input.lastName}`.trim() || null, enabled: input.enabled })
    return id
  }
  async setUserName(userId: string, firstName: string, lastName: string) {
    this.record('setUserName', userId, firstName, lastName)
    const u = this.users.get(userId)
    if (u) u.name = `${firstName} ${lastName}`.trim() || null
  }

  ldap: (KcLdap & { bindPassword?: string }) | null = null
  /** Makes the directory calls answer like Keycloak does when the service account lacks `manage-realm`. */
  forbidLdap = false
  async getLdap() {
    if (this.forbidLdap) throw new HttpProblem(502, 'keycloak_forbidden', 'Keycloak refused the API\'s service account')
    if (!this.ldap) return null
    const { bindPassword: _p, ...ldap } = this.ldap
    return ldap
  }
  async setLdap(input: LdapInput) {
    this.record('setLdap', input.vendor, input.url)
    this.ldap = {
      vendor: input.vendor,
      url: input.url,
      startTls: input.startTls,
      bindDn: input.bindDn,
      usersDn: input.usersDn,
      userFilter: input.userFilter ?? '',
      enabled: input.enabled,
      bindPassword: input.bindPassword ?? this.ldap?.bindPassword,
    }
  }
  async deleteLdap() {
    this.record('deleteLdap')
    this.ldap = null
  }
  async testLdap(input: LdapInput) {
    this.record('testLdap', input.url)
  }
  async syncLdap() {
    this.record('syncLdap')
    return { added: 2, updated: 1, removed: 0, failed: 0 }
  }
}
