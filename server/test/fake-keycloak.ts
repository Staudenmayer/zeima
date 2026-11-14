// A Keycloak admin client that only records what it was asked to do.

import { randomUUID } from 'node:crypto'
import type { KcDepartment, KcUser, KeycloakAdmin } from '../src/keycloak/admin.ts'
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
}
