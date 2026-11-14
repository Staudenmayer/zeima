import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands } from '@/bindings'
import { call } from '@/lib/ipc'

export type OrgRole = 'owner' | 'admin' | 'manager' | 'member'

export interface OrgSummary {
  id: string
  role: OrgRole
}
export interface OrgInfo {
  id: string
  name: string
  role: OrgRole
  seats: { limit: number | null; members: number; pending: number }
}
export interface Member {
  id: string
  email: string | null
  name: string | null
  role: OrgRole
  active: boolean
  lastSeenAt: number | null
}
export interface Invitation {
  id: string
  email: string
  role: 'admin' | 'manager' | 'member'
  createdAt: number
}
export interface Department {
  id: string
  name: string
  path: string
  parentId: string | null
  memberIds: string[]
}
export interface PresenceItem {
  userId: string
  state: 'online' | 'tracking' | 'in_meeting' | 'away'
  projectId: string | null
  since: number
}
export interface AuditEntry {
  id: number
  at: number
  actorId: string | null
  action: string
  target: string | null
  detail: Record<string, unknown>
}

/** What a role may do (packages/shared-fixtures/permissions.yaml; the server decides). */
export const CAN = {
  invite: (r: OrgRole) => r === 'owner' || r === 'admin',
  manage: (r: OrgRole) => r === 'owner' || r === 'admin',
  assignRoles: (r: OrgRole) => r === 'owner',
  audit: (r: OrgRole) => r === 'owner' || r === 'admin',
  delete: (r: OrgRole) => r === 'owner',
}

/** The organization the signed-in person administers: members, invitations, departments. */
export const useOrgStore = defineStore('org', () => {
  const orgs = ref<OrgSummary[]>([])
  const activeId = ref<string | null>(null)
  const info = ref<OrgInfo | null>(null)
  const members = ref<Member[]>([])
  const invitations = ref<Invitation[]>([])
  const departments = ref<Department[]>([])
  const audit = ref<AuditEntry[]>([])
  const auditDone = ref(false)
  const presence = ref<PresenceItem[]>([])
  const loading = ref(false)

  const role = computed<OrgRole | null>(() => orgs.value.find((o) => o.id === activeId.value)?.role ?? null)

  /** Request to the server; the answer is parsed JSON, or null for "no content". */
  async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
    const text = await call(commands.orgRequest(method, path, body === undefined ? null : JSON.stringify(body)))
    return (text ? JSON.parse(text) : null) as T
  }

  const base = () => `/v1/orgs/${activeId.value}`

  async function loadOrgs() {
    const me = await api<{ orgs: OrgSummary[] }>('GET', '/v1/me')
    orgs.value = me.orgs
    if (!orgs.value.some((o) => o.id === activeId.value)) activeId.value = orgs.value[0]?.id ?? null
  }

  async function load() {
    loading.value = true
    try {
      await loadOrgs()
      if (!activeId.value) return
      info.value = await api<OrgInfo>('GET', base())
      const r = role.value!
      const [m, g, i] = await Promise.all([
        api<Member[]>('GET', `${base()}/members`),
        api<Department[]>('GET', `${base()}/groups`),
        CAN.invite(r) ? api<Invitation[]>('GET', `${base()}/invitations`) : Promise.resolve([]),
      ])
      members.value = m
      departments.value = g
      invitations.value = i
      audit.value = []
      auditDone.value = false
      if (CAN.audit(r)) await loadMoreAudit()
    } finally {
      loading.value = false
    }
  }

  async function loadPresence() {
    if (activeId.value) presence.value = await api<PresenceItem[]>('GET', `${base()}/presence`)
  }

  async function loadMoreAudit() {
    const before = audit.value[audit.value.length - 1]?.id
    const page = await api<AuditEntry[]>('GET', `${base()}/audit?limit=50${before ? `&before=${before}` : ''}`)
    audit.value.push(...page)
    auditDone.value = page.length < 50
  }

  async function create(name: string) {
    const created = await api<{ id: string }>('POST', '/v1/orgs', { name })
    activeId.value = created.id
    await load()
  }

  async function invite(email: string, inviteRole: Invitation['role']) {
    await api('POST', `${base()}/invitations`, { email, role: inviteRole })
    await load()
  }

  async function withdraw(invitationId: string) {
    await api('DELETE', `${base()}/invitations/${invitationId}`)
    await load()
  }

  async function updateMember(userId: string, change: { role?: OrgRole; active?: boolean }) {
    await api('PATCH', `${base()}/members/${userId}`, change)
    await load()
  }

  async function removeMember(userId: string) {
    await api('DELETE', `${base()}/members/${userId}`)
    await load()
  }

  async function createDepartment(name: string, parentId: string | null) {
    await api('POST', `${base()}/groups`, { name, ...(parentId && { parentId }) })
    await load()
  }

  async function deleteDepartment(id: string) {
    await api('DELETE', `${base()}/groups/${id}`)
    await load()
  }

  async function setDepartmentMember(groupId: string, userId: string, member: boolean) {
    await api(member ? 'PUT' : 'DELETE', `${base()}/groups/${groupId}/members/${userId}`)
    await load()
  }

  async function deleteOrganization() {
    await api('DELETE', base())
    activeId.value = null
    info.value = null
    await load()
  }

  return {
    orgs, activeId, info, members, invitations, departments, audit, auditDone, loading, role,
    presence, loadPresence, loadOrgs, load, loadMoreAudit, create, invite, withdraw, updateMember, removeMember,
    createDepartment, deleteDepartment, setDepartmentMember, deleteOrganization,
  }
})
