// A Keycloak admin client that only records what it was asked to do.

import { randomUUID } from 'node:crypto'
import type { KeycloakAdmin } from '../src/keycloak/admin.ts'

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
  async addToGroup(userId: string, groupId: string) {
    this.record('addToGroup', userId, groupId)
  }
  async removeFromGroup(userId: string, groupId: string) {
    this.record('removeFromGroup', userId, groupId)
  }
}
