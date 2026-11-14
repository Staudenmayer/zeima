<script setup lang="ts">
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import PolicyDialog from '@/components/time/PolicyDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { events } from '@/bindings'
import { errorMessage } from '@/lib/ipc'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'
import type { OrgPolicy } from '@/bindings'
import { useAccountStore } from '@/stores/account'
import { CAN, type Department, type Invitation, type Member, type OrgRole, useOrgStore } from '@/stores/org'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'

definePage({ meta: { titleKey: 'nav.organization' } })

const { t } = useI18n()
const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()

const ROLES: OrgRole[] = ['owner', 'admin', 'manager', 'member']
const canInvite = computed(() => !!org.role && CAN.invite(org.role))
const canManage = computed(() => !!org.role && CAN.manage(org.role))
const canAssign = computed(() => !!org.role && CAN.assignRoles(org.role))
const isOwner = computed(() => !!org.role && CAN.delete(org.role))
/** Roles this person may hand out in an invitation. */
const inviteRoles = computed<Invitation['role'][]>(() => (canAssign.value ? ['member', 'manager', 'admin'] : ['member']))

const projects = useProjectsStore()
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
const STATE_COLOR: Record<string, string> = { online: '#22c55e', tracking: '#3b82f6', in_meeting: '#f59e0b', away: '#9ca3af' }
async function load() {
  if (!account.status.signedIn) return
  try {
    await org.load()
    await org.loadPresence().catch(() => {})
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
async function sendInvite() {
  await run(async () => {
    await org.invite(inviteEmail.value.trim(), inviteRole.value)
    inviteOpen.value = false
    inviteEmail.value = ''
    inviteRole.value = 'member'
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
const canEditRole = (m: Member) => canAssign.value && m.id !== account.status.user?.id
const canTouch = (m: Member) => canManage.value && (m.role !== 'owner' || isOwner.value) && m.id !== account.status.user?.id

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
const deleting = ref<Department | null>(null)
const deleteDeptOpen = ref(false)
function askDeleteDept(g: Department) {
  deleting.value = g
  deleteDeptOpen.value = true
}
const memberName = (id: string) => org.nameOf(id) || id
const addable = (g: Department) => org.members.filter((m) => !g.memberIds.includes(m.id))

// ---------------------------------------------------------------- activity
const actionLabel = (a: string) => {
  const key = `org.audit.${a.replace('.', '_')}`
  return t(key) === key ? a : t(key)
}
const actor = (id: string | null) => (id ? memberName(id) : '')

const deleteOrgOpen = ref(false)

// Sortable tables. The activity log is loaded page by page, newest first; its sort covers what is loaded.
const { sort: memberSort, sorted: sortedMembers } = useSort(() => org.members, {
  member: (m) => label(m),
  role: (m) => m.role,
  status: (m) => (m.active ? 0 : 1),
})
const { sort: presenceSort, sorted: sortedPresence } = useSort(() => org.presence, {
  member: (p) => memberName(p.userId),
  project: (p) => (p.projectId ? projects.byId.get(p.projectId)?.name : '') ?? '',
  status: (p) => t(`org.presenceStates.${p.state}`),
})
const { sort: inviteSort, sorted: sortedInvitations } = useSort(() => org.invitations, {
  email: (i) => i.email,
  invited: (i) => i.createdAt,
  role: (i) => i.role,
})
const { sort: policySort, sorted: sortedScopes } = useSort(policyScopes, {
  scope: (s) => s.label,
  rules: (s) => policySummary(policyOf(s.path)),
})
const { sort: auditSort, sorted: sortedAudit } = useSort(() => org.audit, {
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
      <Badge variant="secondary">{{ $t(`org.roles.${org.info.role}`) }}</Badge>
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
        <TabsTrigger value="online">{{ $t('org.tabs.online') }}<span v-if="org.presence.length" class="ml-1 text-muted-foreground">({{ org.presence.length }})</span></TabsTrigger>
        <TabsTrigger v-if="canInvite" value="invitations">
          {{ $t('org.tabs.invitations') }}<span v-if="org.invitations.length" class="ml-1 text-muted-foreground">({{ org.invitations.length }})</span>
        </TabsTrigger>
        <TabsTrigger value="departments">{{ $t('org.tabs.departments') }}</TabsTrigger>
        <TabsTrigger v-if="canManage" value="policies">{{ $t('org.tabs.policies') }}</TabsTrigger>
        <TabsTrigger v-if="org.role && CAN.audit(org.role)" value="activity">{{ $t('org.tabs.activity') }}</TabsTrigger>
      </TabsList>

      <TabsContent value="members">
        <div class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="memberSort" column="member" class="px-3 py-2 font-medium">{{ $t('org.col.member') }}</SortHead>
                <SortHead :sort="memberSort" column="role" class="px-3 py-2 font-medium">{{ $t('org.col.role') }}</SortHead>
                <SortHead :sort="memberSort" column="status" class="px-3 py-2 font-medium">{{ $t('org.col.status') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="m in sortedMembers" :key="m.id" class="group border-b last:border-b-0 hover:bg-muted/30">
                <td class="px-3 py-1.5">
                  <p class="truncate font-medium">{{ label(m) }}</p>
                  <p v-if="m.name && m.email" class="truncate text-xs text-muted-foreground">{{ m.email }}</p>
                </td>
                <td class="px-3 py-1.5">
                  <Select v-if="canEditRole(m)" :model-value="m.role" @update:model-value="(r) => run(() => org.updateMember(m.id, { role: r as OrgRole }))">
                    <SelectTrigger class="h-8 w-32"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem v-for="r in ROLES" :key="r" :value="r">{{ $t(`org.roles.${r}`) }}</SelectItem>
                    </SelectContent>
                  </Select>
                  <Badge v-else variant="secondary">{{ $t(`org.roles.${m.role}`) }}</Badge>
                </td>
                <td class="px-3 py-1.5"><Badge v-if="!m.active" variant="outline">{{ $t('org.deactivated') }}</Badge><span v-else class="text-muted-foreground">{{ $t('org.active') }}</span></td>
                <td class="px-1 py-0.5">
                  <div v-if="canTouch(m)" class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button
                      size="icon" variant="ghost" class="size-7"
                      :title="m.active ? $t('org.deactivate') : $t('org.reactivate')"
                      @click="run(() => org.updateMember(m.id, { active: !m.active }))"
                    >
                      <UserX v-if="m.active" /><UserCheck v-else />
                    </Button>
                    <Button size="icon" variant="ghost" class="size-7" :title="$t('org.remove')" @click="askRemove(m)"><UserMinus /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="isOwner" class="mt-6">
          <Button variant="outline" class="text-destructive" @click="deleteOrgOpen = true"><Trash2 /> {{ $t('org.deleteOrg') }}</Button>
        </div>
      </TabsContent>

      <TabsContent value="online">
        <p v-if="org.presence.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('org.nobodyOnline') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="presenceSort" column="member" class="px-3 py-2 font-medium">{{ $t('org.col.member') }}</SortHead>
                <SortHead :sort="presenceSort" column="project" class="px-3 py-2 font-medium">{{ $t('org.col.project') }}</SortHead>
                <SortHead :sort="presenceSort" column="status" class="px-3 py-2 font-medium">{{ $t('org.col.status') }}</SortHead>
              </tr>
            </thead>
            <tbody>
              <tr v-for="p in sortedPresence" :key="p.userId" class="border-b last:border-b-0 hover:bg-muted/30">
                <td class="px-3 py-1.5 font-medium"><span class="flex items-center gap-2"><span class="size-2.5 shrink-0 rounded-full" :style="{ backgroundColor: STATE_COLOR[p.state] }" />{{ memberName(p.userId) }}</span></td>
                <td class="px-3 py-1.5 text-muted-foreground">{{ p.projectId ? projects.byId.get(p.projectId)?.name : '–' }}</td>
                <td class="px-3 py-1.5 text-muted-foreground">{{ $t(`org.presenceStates.${p.state}`) }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <p class="mt-3 text-xs text-muted-foreground">{{ $t('org.presenceHint') }}</p>
      </TabsContent>

      <TabsContent v-if="canInvite" value="invitations">
        <p v-if="org.invitations.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('org.noInvitations') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="inviteSort" column="email" class="px-3 py-2 font-medium">{{ $t('org.email') }}</SortHead>
                <SortHead :sort="inviteSort" column="invited" class="px-3 py-2 font-medium">{{ $t('org.col.invited') }}</SortHead>
                <SortHead :sort="inviteSort" column="role" class="px-3 py-2 font-medium">{{ $t('org.col.role') }}</SortHead>
                <th class="w-24 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="i in sortedInvitations" :key="i.id" class="group border-b last:border-b-0 hover:bg-muted/30">
                <td class="px-3 py-1.5">{{ i.email }}</td>
                <td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">{{ format.shortDate(i.createdAt) }}</td>
                <td class="px-3 py-1.5"><Badge variant="secondary">{{ $t(`org.roles.${i.role}`) }}</Badge></td>
                <td class="px-1 py-0.5">
                  <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button size="icon" variant="ghost" class="size-7" :title="$t('org.withdraw')" @click="run(() => org.withdraw(i.id))"><X /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <p class="mt-3 text-xs text-muted-foreground">{{ $t('org.invitationHint') }}</p>
      </TabsContent>

      <TabsContent value="departments">
        <div v-if="canManage" class="mb-3"><Button variant="outline" @click="deptOpen = true"><Plus /> {{ $t('org.newDepartment') }}</Button></div>
        <p v-if="org.departments.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('org.noDepartments') }}</p>
        <ul v-else class="grid gap-2">
          <li v-for="g in org.departments" :key="g.id" class="rounded-lg border p-3 text-sm" :style="{ marginLeft: `${(g.path.split('/').length - 1) * 1.5}rem` }">
            <div class="flex items-center gap-2">
              <span class="flex-1 font-medium">{{ g.name }}</span>
              <span class="text-xs text-muted-foreground">{{ $t('org.peopleCount', g.memberIds.length) }}</span>
              <Button v-if="canManage" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDeleteDept(g)"><Trash2 /></Button>
            </div>
            <div class="mt-2 flex flex-wrap items-center gap-1.5">
              <Badge v-for="id in g.memberIds" :key="id" variant="secondary" class="gap-1">
                {{ memberName(id) }}
                <button v-if="canManage" type="button" class="opacity-60 hover:opacity-100" :aria-label="$t('org.remove')" @click="run(() => org.setDepartmentMember(g.id, id, false))"><X class="size-3" /></button>
              </Badge>
              <Select v-if="canManage && addable(g).length" model-value="" @update:model-value="(id) => run(() => org.setDepartmentMember(g.id, String(id), true))">
                <SelectTrigger class="h-7 w-40 text-xs"><SelectValue :placeholder="$t('org.addPerson')" /></SelectTrigger>
                <SelectContent>
                  <SelectItem v-for="m in addable(g)" :key="m.id" :value="m.id">{{ label(m) }}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </li>
        </ul>
      </TabsContent>

      <TabsContent v-if="canManage" value="policies">
        <p v-if="!workspaces.inOrg" class="rounded-md border p-3 text-sm text-muted-foreground">{{ $t('org.policy.switch') }}</p>
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
        <p class="mt-3 text-xs text-muted-foreground">{{ $t('org.policy.hint') }}</p>
      </TabsContent>

      <TabsContent v-if="org.role && CAN.audit(org.role)" value="activity">
        <div class="overflow-x-auto rounded-lg border">
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
              <SelectItem v-for="r in inviteRoles" :key="r" :value="r">{{ $t(`org.roles.${r}`) }}</SelectItem>
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
