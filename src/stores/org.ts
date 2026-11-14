import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type OrgPolicy, type PolicySettings } from '@/bindings'
import { call } from '@/lib/ipc'
import i18n from '@/plugins/i18n'

/** A built-in role's name (`admin`) or the id of a role the organization made. */
export type OrgRole = string

export type Scope = 'own' | 'group' | 'org'
const RANK: Record<Scope, number> = { own: 0, group: 1, org: 2 }
/** What a role allows: action → the widest scope. */
export type Permissions = Record<string, Scope>

/** Whether `have` holds everything in `want`, at least as wide (a role may only hand out what its giver holds). */
export function covers(have: Permissions, want: Permissions): boolean {
  return Object.entries(want).every(([action, scope]) => have[action] !== undefined && RANK[have[action]!] >= RANK[scope])
}

export interface OrgSummary {
  id: string
  role: OrgRole
  permissions: Permissions
}
/** A role of the organization, with what it allows (only for those who view roles, and for one's own). */
export interface Role {
  key: string
  name: string
  description: string
  builtin: boolean
  locked: boolean
  people: number
  permissions: Permissions | null
}
/** What a role can be given: grouped by what it acts on, and by view, edit or delete. */
export interface PermissionDef {
  action: string
  resource: string
  kind: 'view' | 'edit' | 'delete'
  scopes: Scope[]
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
  role: OrgRole
  /** Departments the person is put into when they join. */
  groupIds: string[]
  /** The job description they get when they join. */
  jobId: string | null
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

export interface DirectoryEntry {
  id: string
  name: string | null
  email: string | null
  /** Department paths, e.g. `dev/backend`. */
  departments: string[]
  jobId: string | null
  /** Name of the person's job description. */
  jobTitle: string | null
  jobDescription: string | null
}

/** A job description the organization defines (Organization → Job descriptions). */
export interface Job {
  id: string
  name: string
  description: string
  people: number
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
  /** Names of everyone in the organization (every member may read them). */
  const policies = ref<OrgPolicy[]>([])
  const people = ref<{ id: string; name: string | null }[]>([])
  /** The coworkers: name, email, departments, job title (every member may read them). */
  const directory = ref<DirectoryEntry[]>([])
  /** The job descriptions the organization defines (every member may read them). */
  const jobs = ref<Job[]>([])
  const loading = ref(false)

  const role = computed<OrgRole | null>(() => orgs.value.find((o) => o.id === activeId.value)?.role ?? null)
  /** The roles of the organization, and what a role can be made of. */
  const roles = ref<Role[]>([])
  const catalog = ref<PermissionDef[]>([])
  const permissions = computed<Permissions>(() => orgs.value.find((o) => o.id === activeId.value)?.permissions ?? {})
  /** Whether the person may do `action` in this organization (the server decides; this shows or hides). */
  const can = (action: string) => permissions.value[action] !== undefined
  /** How wide: their own, their departments' or everyone's; null if not at all. */
  const scopeOf = (action: string): Scope | null => permissions.value[action] ?? null

  /** Request to the server; the answer is parsed JSON, or null for "no content". */
  async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
    const text = await call(commands.orgRequest(method, path, body === undefined ? null : JSON.stringify(body)))
    return (text ? JSON.parse(text) : null) as T
  }

  /** A person's name, from what the role may read of the members or else the names list. */
  function nameOf(id: string): string {
    const m = members.value.find((x) => x.id === id)
    return m?.name || m?.email || people.value.find((x) => x.id === id)?.name || ''
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
      await loadRoles().catch(() => {})
      people.value = await api<{ id: string; name: string | null }[]>('GET', `${base()}/people`).catch(() => [])
      const [m, g, i] = await Promise.all([
        api<Member[]>('GET', `${base()}/members`),
        api<Department[]>('GET', `${base()}/groups`),
        can('member.invite') ? api<Invitation[]>('GET', `${base()}/invitations`) : Promise.resolve([]),
      ])
      members.value = m
      departments.value = g
      invitations.value = i
      audit.value = []
      auditDone.value = false
      await Promise.all([loadDirectory().catch(() => {}), loadJobs().catch(() => {})])
      if (can('audit.read')) await loadMoreAudit()
    } finally {
      loading.value = false
    }
  }

  /** Policies live in the organization's workspace: the active workspace must be that one. */
  async function loadPolicies() {
    policies.value = await call(commands.policiesList())
  }
  async function savePolicy(groupPath: string | null, settings: PolicySettings) {
    await call(commands.policySave(groupPath, settings))
    await loadPolicies()
  }
  async function deletePolicy(id: string) {
    await call(commands.policyDelete(id))
    await loadPolicies()
  }

  /** Names and presence only: what a widget needs, without loading the whole organization. */
  async function loadGlance(orgId: string) {
    activeId.value = orgId
    people.value = await api<{ id: string; name: string | null }[]>('GET', `${base()}/people`).catch(() => [])
    await loadDirectory().catch(() => {})
    await loadPresence()
  }

  async function loadDirectory() {
    if (activeId.value) directory.value = await api<DirectoryEntry[]>('GET', `${base()}/directory`)
  }

  async function loadJobs() {
    if (activeId.value) jobs.value = await api<Job[]>('GET', `${base()}/jobs`)
  }
  async function createJob(name: string, description: string) {
    await api('POST', `${base()}/jobs`, { name, description })
    await loadJobs()
  }
  async function updateJob(id: string, name: string, description: string) {
    await api('PATCH', `${base()}/jobs/${id}`, { name, description })
    await Promise.all([loadJobs(), loadDirectory()])
  }
  async function deleteJob(id: string) {
    await api('DELETE', `${base()}/jobs/${id}`)
    await Promise.all([loadJobs(), loadDirectory()])
  }
  /** Gives a person a job description from the list (owners and admins only). */
  async function setJob(userId: string, jobId: string | null) {
    await api('PUT', `${base()}/members/${userId}/job`, { jobId })
    await Promise.all([loadJobs(), loadDirectory()])
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

  async function invite(email: string, inviteRole: Invitation['role'], groupIds: string[] = [], jobId: string | null = null) {
    await api('POST', `${base()}/invitations`, { email, role: inviteRole, groupIds, jobId })
    await load()
  }

  /** Changes the departments and job description a pending invitation gives. */
  async function updateInvitation(invitationId: string, change: { groupIds?: string[]; jobId?: string | null }) {
    await api('PATCH', `${base()}/invitations/${invitationId}`, change)
    await load()
  }

  async function withdraw(invitationId: string) {
    await api('DELETE', `${base()}/invitations/${invitationId}`)
    await load()
  }

  async function loadRoles() {
    if (!activeId.value) return
    const answer = await api<{ roles: Role[]; catalog: PermissionDef[] }>('GET', `${base()}/roles`)
    roles.value = answer.roles
    catalog.value = answer.catalog
  }
  async function createRole(input: { name: string; description: string; permissions: Permissions }) {
    await api('POST', `${base()}/roles`, input)
    await Promise.all([loadRoles(), loadOrgs()])
  }
  async function updateRole(key: string, input: { name?: string; description?: string; permissions?: Permissions }) {
    await api('PATCH', `${base()}/roles/${key}`, input)
    await Promise.all([loadRoles(), loadOrgs()])
  }
  async function deleteRole(key: string) {
    await api('DELETE', `${base()}/roles/${key}`)
    await load()
  }
  /** A role's name: the built-in ones in the person's language, the organization's own as written. */
  function roleName(key: string): string {
    const k = `org.roles.${key}`
    if (i18n.global.te(k)) return i18n.global.t(k)
    return roles.value.find((r) => r.key === key)?.name ?? key
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

  async function renameDepartment(id: string, name: string) {
    await api('PATCH', `${base()}/groups/${id}`, { name })
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
    orgs, activeId, info, members, invitations, departments, audit, auditDone, loading, role, roles, catalog, permissions, can, scopeOf, loadRoles, createRole, updateRole, deleteRole, roleName,
    directory, loadDirectory, updateInvitation, jobs, loadJobs, createJob, updateJob, deleteJob, setJob, loadGlance, policies, loadPolicies, savePolicy, deletePolicy, people, nameOf, presence, loadPresence, loadOrgs, load, loadMoreAudit, create, invite, withdraw, updateMember, removeMember,
    createDepartment, renameDepartment, deleteDepartment, setDepartmentMember, deleteOrganization,
  }
})
