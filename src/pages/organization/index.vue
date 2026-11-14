<script setup lang="ts">
import UserAvatar from '@/components/time/UserAvatar.vue'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Building2, MailPlus, Pencil, Plus, Trash2, UserMinus, UserX, UserCheck, X } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import PolicyDialog from '@/components/time/PolicyDialog.vue'
import MultiSelect from '@/components/time/MultiSelect.vue'
import PresenceDot from '@/components/time/PresenceDot.vue'
import RoleDialog from '@/components/time/RoleDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { events } from '@/bindings'
import { errorMessage } from '@/lib/ipc'
import { useWorkspaceStore } from '@/stores/workspace'
import type { OrgPolicy } from '@/bindings'
import { useAccountStore } from '@/stores/account'
import { covers, type Department, type Invitation, type Job, type Member, type OrgRole, type Role, useOrgStore } from '@/stores/org'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'
import TableToolbar from '@/components/time/TableToolbar.vue'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import { passes, useTableSearch } from '@/composables/useTableSearch'

definePage({ meta: { titleKey: 'nav.organization' } })

const { t } = useI18n()
const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()

// What the person may do comes from their role's permissions (the server decides).
const canInvite = computed(() => org.can('member.invite'))
const canManage = computed(() => org.can('member.manage'))
const canAssign = computed(() => org.can('role.assign'))
const isOwner = computed(() => org.can('org.delete'))
const canEditDepartments = computed(() => org.can('group.manage'))
const canDeleteDepartments = computed(() => org.can('group.delete'))
const canEditJobs = computed(() => org.can('job.edit'))
const canDeleteJobs = computed(() => org.can('job.delete'))
const canSeeRoles = computed(() => org.can('role.view'))
/** Roles this person may hand out in an invitation. */
/** The roles the person may give: only what they hold themselves, and the owner role only an owner. */
const givableRoles = computed(() =>
  org.roles.filter((r) => (r.key === 'owner' ? org.role === 'owner' : r.permissions !== null && covers(org.permissions, r.permissions))),
)
const inviteRoles = computed(() => (canAssign.value ? givableRoles.value : givableRoles.value.filter((r) => r.key === 'member')))

const workspaces = useWorkspaceStore()

// ----------------------------------------------------------------- policies
/** The scopes a policy can be set for: the organization, then each department. */
const policyScopes = computed(() => [{ path: null as string | null, label: t('org.policy.wholeOrg') }, ...org.departments.map((g) => ({ path: g.path as string | null, label: g.path }))])
const policyOf = (path: string | null): OrgPolicy | null => org.policies.find((p) => p.groupPath === path) ?? null
const editingScope = ref<{ path: string | null; label: string } | null>(null)
const policyOpen = ref(false)
function editPolicy(scope: { path: string | null; label: string }) {
  editingScope.value = scope
  policyOpen.value = true
}
/** What a policy asks for, in a few words. */
function policySummary(p: OrgPolicy | null): string {
  if (!p) return t('org.policy.none')
  const parts: string[] = []
  if (p.requireProject) parts.push(t('org.policy.requireProject'))
  if (p.requireTags) parts.push(t('org.policy.requireTags'))
  if (p.managerVisibility && p.managerVisibility !== 'full') parts.push(t(`org.policy.manager.${p.managerVisibility}`))
  if (p.presenceVisibility === 'off') parts.push(t('org.policy.presence.off'))
  if (p.weeklyTargetMinutes) parts.push(t('org.policy.targetShort', { h: p.weeklyTargetMinutes / 60 }))
  return parts.join(' · ') || t('org.policy.none')
}
let unlisten: (() => void) | undefined
let refresh: ReturnType<typeof setInterval> | undefined
onMounted(async () => {
  await load()
  // Others' status changes arrive as events; a quiet minute still ages out stale reports.
  unlisten = await events.presenceChanged.listen((e) => {
    if (e.payload.orgId === org.activeId) void org.loadPresence().catch(() => {})
  })
  refresh = setInterval(() => void org.loadPresence().catch(() => {}), 60_000)
})
onUnmounted(() => {
  unlisten?.()
  clearInterval(refresh)
})
async function load() {
  if (!account.status.signedIn) return
  try {
    await org.load()
    await org.loadPresence().catch(() => {})
    await org.loadJobs().catch(() => {})
    if (workspaces.inOrg) await org.loadPolicies().catch(() => {})
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

/** Runs an action, shows its error and reloads nothing (the store does). */
async function run(action: () => Promise<unknown>, done?: string) {
  try {
    await action()
    if (done) toast.success(done)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

// ----------------------------------------------------------- create / invite
const newName = ref('')
const creating = ref(false)
async function create() {
  if (!newName.value.trim()) return
  creating.value = true
  await run(() => org.create(newName.value.trim()), t('org.created'))
  creating.value = false
  newName.value = ''
}

const inviteOpen = ref(false)
const inviteEmail = ref('')
const inviteRole = ref<Invitation['role']>('member')
const inviteGroups = ref<string[]>([])
const inviteJob = ref('__none')
async function sendInvite() {
  await run(async () => {
    await org.invite(inviteEmail.value.trim(), inviteRole.value, inviteGroups.value, inviteJob.value === '__none' ? null : inviteJob.value)
    inviteOpen.value = false
    inviteEmail.value = ''
    inviteRole.value = 'member'
    inviteGroups.value = []
    inviteJob.value = '__none'
  }, t('org.invited'))
}

// ------------------------------------------------------------------ members
const removing = ref<Member | null>(null)
const removeOpen = ref(false)
function askRemove(m: Member) {
  removing.value = m
  removeOpen.value = true
}
const label = (m: Member) => m.name || m.email || m.id

/** Whether the signed-in person may change this member's role. */
const canEditRole = (m: Member) =>
  canAssign.value && m.id !== account.status.user?.id && givableRoles.value.some((r) => r.key === m.role)
/** Why a role cannot be changed, for the tooltip of the badge that stands in for the list. */
const whyNotRole = (m: Member) =>
  m.id === account.status.user?.id ? t('org.roleOwn') : !canAssign.value ? t('org.roleNoAssign') : t('org.roleHoldsMore')
const canTouch = (m: Member) => (canManage.value || org.can('member.remove')) && (m.role !== 'owner' || isOwner.value) && m.id !== account.status.user?.id

// -------------------------------------------------------------- departments
const deptOpen = ref(false)
const deptName = ref('')
const deptParent = ref<string>('none')
async function createDept() {
  await run(async () => {
    await org.createDepartment(deptName.value.trim(), deptParent.value === 'none' ? null : deptParent.value)
    deptOpen.value = false
    deptName.value = ''
    deptParent.value = 'none'
  })
}
const renamingDept = ref<Department | null>(null)
const renameOpen = ref(false)
const renameTo = ref('')
function openDept(g: Department) {
  renamingDept.value = g
  renameTo.value = g.name
  renameOpen.value = true
}
async function renameDept() {
  const g = renamingDept.value
  if (!g) return
  await run(async () => {
    await org.renameDepartment(g.id, renameTo.value.trim())
    renameOpen.value = false
  })
}
const deleting = ref<Department | null>(null)
const deleteDeptOpen = ref(false)
function askDeleteDept(g: Department) {
  deleting.value = g
  deleteDeptOpen.value = true
}
const memberName = (id: string) => org.nameOf(id) || id

// ---------------------------------------------------------------- job descriptions
const jobOpen = ref(false)
const editingJob = ref<Job | null>(null)
const jobName = ref('')
const jobText = ref('')
function openJob(j: Job | null = null) {
  editingJob.value = j
  jobName.value = j?.name ?? ''
  jobText.value = j?.description ?? ''
  jobOpen.value = true
}
async function saveJob() {
  await run(async () => {
    if (editingJob.value) await org.updateJob(editingJob.value.id, jobName.value.trim(), jobText.value.trim())
    else await org.createJob(jobName.value.trim(), jobText.value.trim())
    jobOpen.value = false
  })
}
const deletingJob = ref<Job | null>(null)
const deleteJobOpen = ref(false)
function askDeleteJob(j: Job) {
  deletingJob.value = j
  deleteJobOpen.value = true
}
const { search: jobSearch, filtered: shownJobs, searching: jobSearching } = useTableSearch(() => org.jobs, (j) => [j.name, j.description])
const { sort: jobSort, sorted: sortedJobs } = useSort(shownJobs, { name: (j) => j.name, description: (j) => j.description, people: (j) => j.people })

// ------------------------------------------------------------------- roles
const roleOpen = ref(false)
const editingRole = ref<Role | null>(null)
function openRole(r: Role | null) {
  editingRole.value = r
  roleOpen.value = true
}
const deletingRole = ref<Role | null>(null)
const deleteRoleOpen = ref(false)
function askDeleteRole(r: Role) {
  deletingRole.value = r
  deleteRoleOpen.value = true
}
const canEditRoles = computed(() => org.can('role.edit'))
const canDeleteRoles = computed(() => org.can('role.delete'))
const { search: roleSearch, filtered: shownRoles, searching: roleSearching } = useTableSearch(() => org.roles, (r) => [org.roleName(r.key), r.description])
const { sort: roleSort, sorted: sortedRoles } = useSort(shownRoles, {
  name: (r) => org.roleName(r.key),
  people: (r) => r.people,
  permissions: (r) => Object.keys(r.permissions ?? {}).length,
})

// ---------------------------------------------------------------- activity
const actionLabel = (a: string) => {
  const key = `org.audit.${a.replace('.', '_')}`
  return t(key) === key ? a : t(key)
}
const actor = (id: string | null) => (id ? memberName(id) : '')

const deleteOrgOpen = ref(false)

// Sortable tables. The activity log is loaded page by page, newest first; its sort covers what is loaded.
// ----------------------------------------- members and invitations, one table
interface PersonRow {
  key: string
  member: Member | null
  invitation: Invitation | null
  name: string
  email: string | null
  role: string
  departments: string[]
  job: string | null
  status: 'active' | 'deactivated' | 'pending'
}
const departmentPaths = (ids: string[]) => org.departments.filter((g) => ids.includes(g.id)).map((g) => g.path).sort()
const people = computed<PersonRow[]>(() => [
  ...org.members.map((m) => ({
    key: `m/${m.id}`,
    member: m,
    invitation: null,
    name: label(m),
    email: m.name ? m.email : null,
    role: m.role,
    departments: org.departments.filter((g) => g.memberIds.includes(m.id)).map((g) => g.path).sort(),
    job: org.directory.find((e) => e.id === m.id)?.jobTitle ?? null,
    status: m.active ? ('active' as const) : ('deactivated' as const),
  })),
  ...org.invitations.map((i) => ({
    key: `i/${i.id}`,
    member: null,
    invitation: i,
    name: i.email,
    email: null,
    role: i.role,
    departments: departmentPaths(i.groupIds),
    job: org.jobs.find((j) => j.id === i.jobId)?.name ?? null,
    status: 'pending' as const,
  })),
])
const STATUS_ORDER = { active: 0, deactivated: 1, pending: 2 }
const memberRoles = ref<(string | null)[]>([])
const memberDepartments = ref<(string | null)[]>([])
const memberJobs = ref<(string | null)[]>([])
const memberStatuses = ref<(string | null)[]>([])
const roleOptions = computed<FilterOption[]>(() => [...new Set(people.value.map((r) => r.role))].map((k) => ({ value: k, label: org.roleName(k) })))
const memberDepartmentOptions = computed<FilterOption[]>(() => [...new Set(people.value.flatMap((r) => r.departments))].sort().map((d) => ({ value: d, label: d })))
const memberJobOptions = computed<FilterOption[]>(() => [...new Set(people.value.flatMap((r) => (r.job ? [r.job] : [])))].sort().map((j) => ({ value: j, label: j })))
const memberStatusOptions = computed<FilterOption[]>(() => [
  { value: 'active', label: t('org.active') },
  { value: 'deactivated', label: t('org.deactivated') },
  { value: 'pending', label: t('org.invitePending') },
])
const { search: memberSearch, filtered: shownPeople, searching: memberSearching } = useTableSearch(
  people,
  (r) => [r.name, r.email, org.roleName(r.role), ...r.departments, r.job],
  (r) => passes(memberRoles.value, r.role) && passes(memberDepartments.value, r.departments) && passes(memberJobs.value, r.job) && passes(memberStatuses.value, r.status),
)
const memberNarrowed = computed(() => memberSearching.value || memberRoles.value.length + memberDepartments.value.length + memberJobs.value.length + memberStatuses.value.length > 0)
const clearMembers = () => {
  memberSearch.value = ''
  memberRoles.value = []
  memberDepartments.value = []
  memberJobs.value = []
  memberStatuses.value = []
}
const { sort: memberSort, sorted: sortedPeople } = useSort(shownPeople, {
  member: (r) => r.name,
  role: (r) => r.role,
  department: (r) => r.departments.join(', '),
  job: (r) => r.job,
  status: (r) => STATUS_ORDER[r.status],
})

// Click a row to set the departments and the job description (of a member, or of
// what an invitation gives when it is accepted).
const personOpen = ref(false)
const editingPerson = ref<PersonRow | null>(null)
const personGroups = ref<string[]>([])
const NO_JOB = '__none'
const personJob = ref(NO_JOB)
function openPerson(r: PersonRow) {
  editingPerson.value = r
  personGroups.value = r.member ? org.departments.filter((g) => g.memberIds.includes(r.member!.id)).map((g) => g.id) : [...(r.invitation?.groupIds ?? [])]
  personJob.value = (r.member ? org.directory.find((e) => e.id === r.member!.id)?.jobId : r.invitation?.jobId) ?? NO_JOB
  personOpen.value = true
}
/** The departments to pick from, nested as they are. */
const departmentOptions = computed(() => org.departments.map((g) => ({ value: g.id, label: g.name, depth: g.path.split('/').length - 1 })))
async function savePerson() {
  const r = editingPerson.value
  if (!r) return
  const jobId = personJob.value === NO_JOB ? null : personJob.value
  await run(async () => {
    if (r.invitation) {
      await org.updateInvitation(r.invitation.id, { groupIds: personGroups.value, jobId })
    } else if (r.member) {
      const had = org.departments.filter((g) => g.memberIds.includes(r.member!.id)).map((g) => g.id)
      for (const id of personGroups.value.filter((g) => !had.includes(g))) await org.setDepartmentMember(id, r.member.id, true)
      for (const id of had.filter((g) => !personGroups.value.includes(g))) await org.setDepartmentMember(id, r.member.id, false)
      await org.setJob(r.member.id, jobId)
    }
    personOpen.value = false
  })
}

const { search: deptSearch, filtered: shownDepartments, searching: deptSearching } = useTableSearch(() => org.departments, (g) => [g.path])
const { sort: deptSort, sorted: sortedDepartments } = useSort(shownDepartments, { name: (g) => g.path, people: (g) => g.memberIds.length })
const { search: policySearch, filtered: shownScopes, searching: policySearching } = useTableSearch(policyScopes, (s) => [s.label])
const { sort: policySort, sorted: sortedScopes } = useSort(shownScopes, {
  scope: (s) => s.label,
  rules: (s) => policySummary(policyOf(s.path)),
})
const auditWho = ref<(string | null)[]>([])
const auditWhat = ref<(string | null)[]>([])
const auditWhoOptions = computed<FilterOption[]>(() => [...new Set(org.audit.flatMap((a) => (a.actorId ? [a.actorId] : [])))].map((id) => ({ value: id, label: actor(id), person: true })))
const auditWhatOptions = computed<FilterOption[]>(() => [...new Set(org.audit.map((a) => a.action))].map((a) => ({ value: a, label: actionLabel(a) })))
const { search: auditSearch, filtered: shownAudit, searching: auditSearching } = useTableSearch(
  () => org.audit,
  (a) => [actionLabel(a.action), actor(a.actorId), a.target],
  (a) => passes(auditWho.value, a.actorId) && passes(auditWhat.value, a.action),
)
const auditNarrowed = computed(() => auditSearching.value || auditWho.value.length > 0 || auditWhat.value.length > 0)
const clearAudit = () => {
  auditSearch.value = ''
  auditWho.value = []
  auditWhat.value = []
}
const { sort: auditSort, sorted: sortedAudit } = useSort(shownAudit, {
  when: (a) => a.at,
  what: (a) => actionLabel(a.action),
  who: (a) => actor(a.actorId),
})
</script>

<template>
  <PageHeader :title="$t('nav.organization')">
    <Button v-if="org.info && canInvite" @click="inviteOpen = true"><MailPlus /> {{ $t('org.invite') }}</Button>
  </PageHeader>

  <EmptyState v-if="!account.status.signedIn" :icon="Building2" :text="$t('org.signedOut')">
    <Button variant="outline" as-child><RouterLink to="/settings">{{ $t('org.toSettings') }}</RouterLink></Button>
  </EmptyState>

  <div v-else-if="!org.loading && org.orgs.length === 0" class="mx-auto grid max-w-md gap-3 rounded-lg border p-5">
    <h2 class="font-medium">{{ $t('org.createTitle') }}</h2>
    <p class="text-sm text-muted-foreground">{{ $t('org.createHint') }}</p>
    <form class="flex gap-2" @submit.prevent="create">
      <Input v-model="newName" :placeholder="$t('org.namePlaceholder')" maxlength="100" />
      <Button type="submit" :disabled="creating || !newName.trim()"><Plus /> {{ $t('common.create') }}</Button>
    </form>
  </div>

  <div v-else-if="org.info" class="grid gap-4">
    <div class="flex flex-wrap items-center gap-3 text-sm">
      <h2 class="text-lg font-medium">{{ org.info.name }}</h2>
      <Badge variant="secondary">{{ org.roleName(org.info.role) }}</Badge>
      <span class="text-muted-foreground">
        {{
          org.info.seats.limit === null
            ? $t('org.seatsUnlimited', { n: org.info.seats.members })
            : $t('org.seats', { used: org.info.seats.members + org.info.seats.pending, limit: org.info.seats.limit })
        }}
      </span>
    </div>

    <Tabs default-value="members">
      <TabsList>
        <TabsTrigger value="members">{{ $t('org.tabs.members') }}</TabsTrigger>
        <TabsTrigger value="departments">{{ $t('org.tabs.departments') }}</TabsTrigger>
        <TabsTrigger v-if="canEditJobs || canDeleteJobs" value="jobs">{{ $t('org.tabs.jobs') }}</TabsTrigger>
        <TabsTrigger v-if="canSeeRoles" value="roles">{{ $t('org.tabs.roles') }}</TabsTrigger>
        <TabsTrigger v-if="org.can('policy.write')" value="policies">{{ $t('org.tabs.policies') }}</TabsTrigger>
        <TabsTrigger v-if="org.can('audit.read')" value="activity">{{ $t('org.tabs.activity') }}</TabsTrigger>
      </TabsList>

      <TabsContent value="members">
        <p v-if="!org.can('member.read')" class="rounded-md border p-3 text-sm text-muted-foreground">{{ $t('noAccess.members') }}</p>
        <template v-else>
        <TableToolbar v-model="memberSearch" :active="memberNarrowed" @clear="clearMembers">
          <MultiFilter v-model="memberRoles" :label="$t('org.col.role')" :options="roleOptions" />
          <MultiFilter v-if="memberDepartmentOptions.length" v-model="memberDepartments" :label="$t('team.department')" :options="memberDepartmentOptions" />
          <MultiFilter v-if="memberJobOptions.length" v-model="memberJobs" :label="$t('directory.jobTitle')" :options="memberJobOptions" />
          <MultiFilter v-model="memberStatuses" :label="$t('org.col.status')" :options="memberStatusOptions" />
        </TableToolbar>
        <p v-if="sortedPeople.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="memberSort" column="member" class="px-3 py-2 font-medium">{{ $t('org.col.member') }}</SortHead>
                <SortHead :sort="memberSort" column="role" class="px-3 py-2 font-medium">{{ $t('org.col.role') }}</SortHead>
                <SortHead :sort="memberSort" column="department" class="px-3 py-2 font-medium">{{ $t('team.department') }}</SortHead>
                <SortHead :sort="memberSort" column="job" class="px-3 py-2 font-medium">{{ $t('directory.jobTitle') }}</SortHead>
                <SortHead :sort="memberSort" column="status" class="px-3 py-2 font-medium">{{ $t('org.col.status') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="r in sortedPeople"
                :key="r.key"
                class="group border-b last:border-b-0 hover:bg-muted/30"
                :class="canManage ? 'cursor-pointer' : ''"
                @click="canManage && openPerson(r)"
              >
                <td class="px-3 py-1.5">
                  <div class="flex items-center gap-2">
                    <span v-if="r.member" class="relative inline-flex shrink-0"><UserAvatar :user-id="r.member.id" :name="r.name" size="md" /><PresenceDot :user-id="r.member.id" class="absolute -right-0.5 -bottom-0.5 ring-2 ring-background" /></span>
                    <UserAvatar v-else :name="r.name" size="md" class="opacity-50" />
                    <div class="min-w-0">
                      <p class="truncate font-medium">{{ r.name }}</p>
                      <p v-if="r.name !== r.email && r.email" class="truncate text-xs text-muted-foreground">{{ r.email }}</p>
                    </div>
                  </div>
                </td>
                <td class="px-3 py-1.5" @click.stop>
                  <Select v-if="r.member && canEditRole(r.member)" :model-value="r.member.role" @update:model-value="(role) => run(() => org.updateMember(r.member!.id, { role: role as OrgRole }))">
                    <SelectTrigger class="h-8 w-32"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem v-for="role in givableRoles" :key="role.key" :value="role.key">{{ org.roleName(role.key) }}</SelectItem>
                    </SelectContent>
                  </Select>
                  <Badge v-else variant="secondary" :title="r.member ? whyNotRole(r.member) : undefined">{{ org.roleName(r.role) }}</Badge>
                </td>
                <td class="px-3 py-1.5">
                  <span v-if="r.departments.length" class="flex flex-wrap gap-1"><Badge v-for="d in r.departments" :key="d" variant="secondary">{{ d }}</Badge></span>
                  <span v-else class="text-muted-foreground">–</span>
                </td>
                <td class="px-3 py-1.5" :class="r.job ? '' : 'text-muted-foreground'">{{ r.job || '–' }}</td>
                <td class="whitespace-nowrap px-3 py-1.5">
                  <Badge v-if="r.status === 'pending'" variant="outline">{{ $t('org.invitePending') }}</Badge>
                  <Badge v-else-if="r.status === 'deactivated'" variant="outline">{{ $t('org.deactivated') }}</Badge>
                  <span v-else class="text-muted-foreground">{{ $t('org.active') }}</span>
                </td>
                <td class="px-1 py-0.5" @click.stop>
                  <div v-if="r.member && canTouch(r.member)" class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button
                      size="icon" variant="ghost" class="size-7"
                      :title="r.member.active ? $t('org.deactivate') : $t('org.reactivate')"
                      @click="run(() => org.updateMember(r.member!.id, { active: !r.member!.active }))"
                    >
                      <UserX v-if="r.member.active" /><UserCheck v-else />
                    </Button>
                    <Button size="icon" variant="ghost" class="size-7" :title="$t('org.remove')" @click="askRemove(r.member)"><UserMinus /></Button>
                  </div>
                  <div v-else-if="r.invitation && canInvite" class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button size="icon" variant="ghost" class="size-7" :title="$t('org.withdraw')" @click="run(() => org.withdraw(r.invitation!.id))"><X /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        </template>
        <p v-if="org.invitations.length" class="mt-3 text-xs text-muted-foreground">{{ $t('org.invitationHint') }}</p>
        <div v-if="isOwner" class="mt-6">
          <Button variant="outline" class="text-destructive" @click="deleteOrgOpen = true"><Trash2 /> {{ $t('org.deleteOrg') }}</Button>
        </div>
      </TabsContent>

      <TabsContent value="departments">
        <div class="mb-3 flex items-center justify-between gap-3">
          <p class="text-sm text-muted-foreground">{{ $t('org.departmentsHint') }}</p>
          <Button v-if="canEditDepartments" size="sm" @click="deptOpen = true"><Plus /> {{ $t('org.newDepartment') }}</Button>
        </div>
        <p v-if="org.departments.length === 0" class="rounded-md border p-3 text-sm text-muted-foreground">{{ $t('org.noDepartments') }}</p>
        <template v-else>
        <TableToolbar v-model="deptSearch" :active="deptSearching" @clear="deptSearch = ''" />
        <p v-if="sortedDepartments.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="deptSort" column="name" class="px-3 py-2 font-medium">{{ $t('team.department') }}</SortHead>
                <SortHead :sort="deptSort" column="people" class="px-3 py-2 text-right font-medium">{{ $t('org.jobs.people') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.delete') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="g in sortedDepartments" :key="g.id" class="group border-b last:border-b-0 hover:bg-muted/30" :class="canEditDepartments ? 'cursor-pointer' : ''" @click="canEditDepartments && openDept(g)">
                <td class="px-3 py-1.5 font-medium" :style="{ paddingLeft: deptSort.key ? undefined : `calc(0.75rem + ${g.path.split('/').length - 1}rem)` }">{{ deptSort.key ? g.path : g.name }}</td>
                <td class="whitespace-nowrap px-3 py-1.5 text-right tabular-nums text-muted-foreground">{{ g.memberIds.length }}</td>
                <td class="px-1 py-0.5" @click.stop>
                  <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button v-if="canEditDepartments" size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="openDept(g)"><Pencil /></Button>
                    <Button v-if="canDeleteDepartments" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDeleteDept(g)"><Trash2 /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        </template>
      </TabsContent>

      <TabsContent v-if="canEditJobs || canDeleteJobs" value="jobs">
        <div class="mb-3 flex items-center justify-between gap-3">
          <p class="text-sm text-muted-foreground">{{ $t('org.jobs.hint') }}</p>
          <Button v-if="canEditJobs" size="sm" @click="openJob()"><Plus /> {{ $t('org.jobs.new') }}</Button>
        </div>
        <p v-if="org.jobs.length === 0" class="rounded-md border p-3 text-sm text-muted-foreground">{{ $t('org.jobs.none') }}</p>
        <template v-else>
        <TableToolbar v-model="jobSearch" :active="jobSearching" @clear="jobSearch = ''" />
        <p v-if="sortedJobs.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="jobSort" column="name" class="px-3 py-2 font-medium">{{ $t('org.jobs.name') }}</SortHead>
                <SortHead :sort="jobSort" column="description" class="px-3 py-2 font-medium">{{ $t('org.jobs.description') }}</SortHead>
                <SortHead :sort="jobSort" column="people" class="px-3 py-2 text-right font-medium">{{ $t('org.jobs.people') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="j in sortedJobs" :key="j.id" class="group border-b last:border-b-0 hover:bg-muted/30" :class="canEditJobs ? 'cursor-pointer' : ''" @click="canEditJobs && openJob(j)">
                <td class="whitespace-nowrap px-3 py-1.5 font-medium">{{ j.name }}</td>
                <td class="max-w-0 w-full truncate px-3 py-1.5 text-muted-foreground" :title="j.description">{{ j.description || '–' }}</td>
                <td class="whitespace-nowrap px-3 py-1.5 text-right tabular-nums text-muted-foreground">{{ j.people }}</td>
                <td class="px-1 py-0.5" @click.stop>
                  <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button v-if="canEditJobs" size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="openJob(j)"><Pencil /></Button>
                    <Button v-if="canDeleteJobs" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDeleteJob(j)"><Trash2 /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        </template>
      </TabsContent>

      <TabsContent v-if="canSeeRoles" value="roles">
        <div class="mb-3 flex items-center justify-between gap-3">
          <p class="text-sm text-muted-foreground">{{ $t('org.rolesHint') }}</p>
          <Button v-if="canEditRoles" size="sm" @click="openRole(null)"><Plus /> {{ $t('org.roleNew') }}</Button>
        </div>
        <TableToolbar v-model="roleSearch" :active="roleSearching" @clear="roleSearch = ''" />
        <p v-if="sortedRoles.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="roleSort" column="name" class="px-3 py-2 font-medium">{{ $t('org.col.role') }}</SortHead>
                <SortHead :sort="roleSort" column="permissions" class="px-3 py-2 text-right font-medium">{{ $t('org.permCount') }}</SortHead>
                <SortHead :sort="roleSort" column="people" class="px-3 py-2 text-right font-medium">{{ $t('org.jobs.people') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in sortedRoles" :key="r.key" class="group cursor-pointer border-b last:border-b-0 hover:bg-muted/30" @click="openRole(r)">
                <td class="px-3 py-1.5">
                  <p class="font-medium">{{ org.roleName(r.key) }} <Badge v-if="r.builtin" variant="outline" class="ml-1">{{ $t('org.roleBuiltin') }}</Badge></p>
                  <p v-if="r.description" class="truncate text-xs text-muted-foreground">{{ r.description }}</p>
                </td>
                <td class="whitespace-nowrap px-3 py-1.5 text-right tabular-nums text-muted-foreground">{{ r.permissions ? Object.keys(r.permissions).length : '–' }}</td>
                <td class="whitespace-nowrap px-3 py-1.5 text-right tabular-nums text-muted-foreground">{{ r.people }}</td>
                <td class="px-1 py-0.5" @click.stop>
                  <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button size="icon" variant="ghost" class="size-7" :title="canEditRoles && !r.locked ? $t('common.edit') : $t('org.roleView')" @click="openRole(r)"><Pencil /></Button>
                    <Button v-if="canDeleteRoles && !r.builtin" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDeleteRole(r)"><Trash2 /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </TabsContent>

      <TabsContent v-if="org.can('policy.write')" value="policies">
        <p v-if="!workspaces.inOrg" class="rounded-md border p-3 text-sm text-muted-foreground">{{ $t('org.policy.switch') }}</p>
        <template v-else>
        <TableToolbar v-model="policySearch" :active="policySearching" @clear="policySearch = ''" />
        <p v-if="sortedScopes.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="policySort" column="scope" class="px-3 py-2 font-medium">{{ $t('org.col.scope') }}</SortHead>
                <SortHead :sort="policySort" column="rules" class="px-3 py-2 font-medium">{{ $t('org.col.rules') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="s in sortedScopes" :key="s.path ?? 'org'" class="group cursor-pointer border-b last:border-b-0 hover:bg-muted/30" @click="editPolicy(s)">
                <td class="whitespace-nowrap px-3 py-1.5 font-medium">{{ s.label }}</td>
                <td class="max-w-0 w-full truncate px-3 py-1.5 text-muted-foreground">{{ policySummary(policyOf(s.path)) }}</td>
                <td class="px-1 py-0.5" @click.stop>
                  <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="editPolicy(s)"><Pencil /></Button>
                    <Button v-if="policyOf(s.path)" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="run(() => org.deletePolicy(policyOf(s.path)!.id))"><Trash2 /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        </template>
        <p class="mt-3 text-xs text-muted-foreground">{{ $t('org.policy.hint') }}</p>
      </TabsContent>

      <TabsContent v-if="org.can('audit.read')" value="activity">
        <TableToolbar v-model="auditSearch" :active="auditNarrowed" @clear="clearAudit">
          <MultiFilter v-model="auditWhat" :label="$t('org.col.what')" :options="auditWhatOptions" />
          <MultiFilter v-model="auditWho" :label="$t('org.col.who')" :options="auditWhoOptions" />
        </TableToolbar>
        <p v-if="sortedAudit.length === 0 && org.audit.length > 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="auditSort" column="when" class="px-3 py-2 font-medium">{{ $t('org.col.when') }}</SortHead>
                <SortHead :sort="auditSort" column="what" class="px-3 py-2 font-medium">{{ $t('org.col.what') }}</SortHead>
                <SortHead :sort="auditSort" column="who" class="px-3 py-2 font-medium">{{ $t('org.col.who') }}</SortHead>
              </tr>
            </thead>
            <tbody>
              <tr v-for="a in sortedAudit" :key="a.id" class="border-b last:border-b-0 hover:bg-muted/30">
                <td class="whitespace-nowrap px-3 py-1.5 tabular-nums text-muted-foreground">{{ format.shortDate(a.at) }} {{ format.time(a.at) }}</td>
                <td class="px-3 py-1.5">{{ actionLabel(a.action) }}<span v-if="a.target" class="text-muted-foreground"> · {{ a.target.length === 36 ? memberName(a.target) : a.target }}</span></td>
                <td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">{{ actor(a.actorId) }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="!org.auditDone" class="mt-3 text-center">
          <Button variant="outline" size="sm" @click="run(() => org.loadMoreAudit())">{{ $t('org.loadMore') }}</Button>
        </div>
      </TabsContent>
    </Tabs>
  </div>

  <Dialog v-model:open="inviteOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('org.invite') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="sendInvite">
        <div class="grid gap-2">
          <Label for="invite-email">{{ $t('org.email') }}</Label>
          <Input id="invite-email" v-model="inviteEmail" type="email" required autofocus />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('org.role') }}</Label>
          <Select v-model="inviteRole">
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="r in inviteRoles" :key="r.key" :value="r.key">{{ org.roleName(r.key) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div v-if="org.departments.length" class="grid gap-2">
          <Label>{{ $t('org.tabs.departments') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <MultiSelect v-model="inviteGroups" :options="departmentOptions" :placeholder="$t('org.pickDepartments')" :search-placeholder="$t('directory.search')" />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('directory.jobTitle') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <Select v-model="inviteJob">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="__none">{{ $t('common.none') }}</SelectItem>
              <SelectItem v-for="j in org.jobs" :key="j.id" :value="j.id">{{ j.name }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="inviteOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('org.sendInvite') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="jobOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ editingJob ? $t('org.jobs.edit') : $t('org.jobs.new') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="saveJob">
        <div class="grid gap-2">
          <Label for="job-name">{{ $t('org.jobs.name') }}</Label>
          <Input id="job-name" v-model="jobName" required maxlength="100" autofocus />
        </div>
        <div class="grid gap-2">
          <Label for="job-desc">{{ $t('org.jobs.description') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <Textarea id="job-desc" v-model="jobText" maxlength="500" rows="3" />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="jobOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <ConfirmDialog
    v-model:open="deleteJobOpen"
    :title="$t('org.jobs.deleteConfirm', { name: deletingJob?.name ?? '' })"
    @confirm="deletingJob && run(() => org.deleteJob(deletingJob!.id))"
  />

  <RoleDialog v-model:open="roleOpen" :role="editingRole" />
  <ConfirmDialog
    v-model:open="deleteRoleOpen"
    :title="$t('org.roleDeleteConfirm', { name: deletingRole?.name ?? '' })"
    @confirm="deletingRole && run(() => org.deleteRole(deletingRole!.key))"
  />

  <Dialog v-model:open="personOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ editingPerson?.name }}</DialogTitle>
        <p v-if="editingPerson?.invitation" class="text-sm text-muted-foreground">{{ $t('org.invitePendingHint') }}</p>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="savePerson">
        <div v-if="org.departments.length" class="grid gap-2">
          <Label>{{ $t('org.tabs.departments') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <MultiSelect v-model="personGroups" :options="departmentOptions" :placeholder="$t('org.pickDepartments')" :search-placeholder="$t('directory.search')" />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('directory.jobTitle') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <Select v-model="personJob">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="__none">{{ $t('common.none') }}</SelectItem>
              <SelectItem v-for="j in org.jobs" :key="j.id" :value="j.id">{{ j.name }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="personOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="renameOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('org.renameDepartment') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="renameDept">
        <div class="grid gap-2">
          <Label for="dept-rename">{{ $t('org.departmentName') }}</Label>
          <Input id="dept-rename" v-model="renameTo" required maxlength="60" autofocus />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="renameOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="deptOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('org.newDepartment') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="createDept">
        <div class="grid gap-2">
          <Label for="dept-name">{{ $t('org.departmentName') }}</Label>
          <Input id="dept-name" v-model="deptName" required maxlength="60" autofocus />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('org.parent') }}</Label>
          <Select v-model="deptParent">
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="none">{{ $t('common.none') }}</SelectItem>
              <SelectItem v-for="g in org.departments" :key="g.id" :value="g.id">{{ g.path }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="deptOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.create') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <PolicyDialog
    v-model:open="policyOpen"
    :title="$t('org.policy.editTitle', { scope: editingScope?.label ?? '' })"
    :policy="editingScope ? policyOf(editingScope.path) : null"
    :save="(settings) => org.savePolicy(editingScope?.path ?? null, settings)"
  />
  <ConfirmDialog
    v-model:open="removeOpen"
    :title="$t('org.removeConfirm', { name: removing ? label(removing) : '' })"
    :description="$t('org.removeHint')"
    @confirm="removing && run(() => org.removeMember(removing!.id))"
  />
  <ConfirmDialog
    v-model:open="deleteDeptOpen"
    :title="$t('org.deleteDepartmentConfirm', { name: deleting?.path ?? '' })"
    :description="$t('org.deleteDepartmentHint')"
    @confirm="deleting && run(() => org.deleteDepartment(deleting!.id))"
  />
  <ConfirmDialog
    v-model:open="deleteOrgOpen"
    :title="$t('org.deleteOrgConfirm', { name: org.info?.name ?? '' })"
    :description="$t('org.deleteOrgHint')"
    @confirm="run(() => org.deleteOrganization())"
  />
</template>
