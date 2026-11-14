// The Keycloak Admin REST API, as far as organization management needs it
// (SPEC §5.9, §7.2). Writes go to Keycloak first; the API's mirror tables
// follow. The API signs in as the service account `zeima-api-admin`.
//
// Layout in Keycloak, per organization (token `groups` claim, auth.ts):
//   Keycloak Organization <orgId>      members, invitations
//   /orgs/<orgId>/roles/<role>         one group per role; a user is in one
//   /orgs/<orgId>/groups/<…>           departments

import type { Config } from '../config.ts'
import type { OrgRole } from '../plugins/auth.ts'
import { ORG_ROLES } from '../plugins/auth.ts'
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
  setRole(orgId: string, userId: string, role: OrgRole): Promise<void>
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
  listRoleMembers(orgId: string): Promise<Record<OrgRole, string[]>>
  deleteGroup(groupId: string): Promise<void>
  addToGroup(userId: string, groupId: string): Promise<void>
  removeFromGroup(userId: string, groupId: string): Promise<void>
}

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

  constructor(admin: NonNullable<Config['keycloak']['admin']>, fetchImpl: typeof fetch = fetch) {
    this.admin = admin
    this.fetchImpl = fetchImpl
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
      throw new HttpProblem(502, 'keycloak_unreachable', `cannot reach Keycloak: ${(err as Error).message}`)
    })
    if (!res.ok) throw new HttpProblem(502, 'keycloak_auth_failed', `Keycloak refused the API's admin client (${res.status})`)
    const body = (await res.json()) as { access_token: string; expires_in: number }
    this.token = { value: body.access_token, expiresAt: now + body.expires_in * 1000 }
    return body.access_token
  }

  /** One call; Keycloak errors become problems (409 stays 409). */
  private async call(method: string, path: string, init: { json?: unknown; form?: Record<string, string> } = {}): Promise<Response> {
    const headers: Record<string, string> = { authorization: `Bearer ${await this.accessToken()}` }
    let body: string | URLSearchParams | undefined
    if (init.json !== undefined) {
      headers['content-type'] = 'application/json'
      body = JSON.stringify(init.json)
    } else if (init.form) {
      headers['content-type'] = 'application/x-www-form-urlencoded'
      body = new URLSearchParams(init.form)
    }
    const res = await this.fetchImpl(`${this.admin.apiUrl}${path}`, { method, headers, body, signal: AbortSignal.timeout(15_000) }).catch(
      (err: unknown) => {
        throw new HttpProblem(502, 'keycloak_unreachable', `cannot reach Keycloak: ${(err as Error).message}`)
      },
    )
    if (res.ok) return res
    const detail = await res.text().catch(() => '')
    if (res.status === 404) throw new HttpProblem(404, 'not_found', 'not found in Keycloak')
    if (res.status === 409) throw new HttpProblem(409, 'conflict', keycloakMessage(detail) || 'already exists')
    if (res.status === 400) throw new HttpProblem(400, 'bad_request', keycloakMessage(detail) || 'Keycloak rejected the request')
    throw new HttpProblem(502, 'keycloak_error', `Keycloak answered ${res.status} for ${method} ${path}: ${keycloakMessage(detail) || 'no details'}`)
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

  async setRole(orgId: string, userId: string, role: OrgRole): Promise<void> {
    for (const other of ORG_ROLES) {
      const id = await this.groupId(['orgs', orgId, 'roles', other], other === role)
      if (!id) continue
      await this.call(other === role ? 'PUT' : 'DELETE', `/users/${userId}/groups/${id}`).catch((err: unknown) => {
        if (other !== role && err instanceof HttpProblem && err.status === 404) return
        throw err
      })
    }
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

  async listRoleMembers(orgId: string): Promise<Record<OrgRole, string[]>> {
    const out = { owner: [], admin: [], manager: [], member: [] } as Record<OrgRole, string[]>
    for (const role of ORG_ROLES) {
      const id = await this.groupId(['orgs', orgId, 'roles', role], false)
      if (id) out[role] = (await this.all<{ id: string }>(`/groups/${id}/members`)).map((m) => m.id)
    }
    return out
  }

  async deleteGroup(groupId: string): Promise<void> {
    await this.call('DELETE', `/groups/${groupId}`)
  }

  async addToGroup(userId: string, groupId: string): Promise<void> {
    await this.call('PUT', `/users/${userId}/groups/${groupId}`)
  }

  async removeFromGroup(userId: string, groupId: string): Promise<void> {
    await this.call('DELETE', `/users/${userId}/groups/${groupId}`)
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
