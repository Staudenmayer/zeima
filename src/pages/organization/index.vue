<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Building2, MailPlus, Plus, Trash2, UserMinus, UserX, UserCheck, X } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { errorMessage } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'
import { CAN, type Department, type Invitation, type Member, type OrgRole, useOrgStore } from '@/stores/org'

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

onMounted(load)
async function load() {
  if (!account.status.signedIn) return
  try {
    await org.load()
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
const memberName = (id: string) => org.members.find((m) => m.id === id)?.name ?? org.members.find((m) => m.id === id)?.email ?? id
const addable = (g: Department) => org.members.filter((m) => !g.memberIds.includes(m.id))

// ---------------------------------------------------------------- activity
const actionLabel = (a: string) => {
  const key = `org.audit.${a.replace('.', '_')}`
  return t(key) === key ? a : t(key)
}
const actor = (id: string | null) => (id ? memberName(id) : '')

const deleteOrgOpen = ref(false)
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
        <TabsTrigger v-if="canInvite" value="invitations">
          {{ $t('org.tabs.invitations') }}<span v-if="org.invitations.length" class="ml-1 text-muted-foreground">({{ org.invitations.length }})</span>
        </TabsTrigger>
        <TabsTrigger value="departments">{{ $t('org.tabs.departments') }}</TabsTrigger>
        <TabsTrigger v-if="org.role && CAN.audit(org.role)" value="activity">{{ $t('org.tabs.activity') }}</TabsTrigger>
      </TabsList>

      <TabsContent value="members">
        <ul class="divide-y rounded-lg border">
          <li v-for="m in org.members" :key="m.id" class="flex items-center gap-3 px-3 py-2 text-sm">
            <div class="min-w-0 flex-1">
              <p class="truncate font-medium">{{ label(m) }}</p>
              <p v-if="m.name && m.email" class="truncate text-xs text-muted-foreground">{{ m.email }}</p>
            </div>
            <Badge v-if="!m.active" variant="outline">{{ $t('org.deactivated') }}</Badge>
            <Select v-if="canEditRole(m)" :model-value="m.role" @update:model-value="(r) => run(() => org.updateMember(m.id, { role: r as OrgRole }))">
              <SelectTrigger class="h-8 w-32"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="r in ROLES" :key="r" :value="r">{{ $t(`org.roles.${r}`) }}</SelectItem>
              </SelectContent>
            </Select>
            <Badge v-else variant="secondary">{{ $t(`org.roles.${m.role}`) }}</Badge>
            <div v-if="canTouch(m)" class="flex">
              <Button
                size="icon" variant="ghost" class="size-7"
                :title="m.active ? $t('org.deactivate') : $t('org.reactivate')"
                @click="run(() => org.updateMember(m.id, { active: !m.active }))"
              >
                <UserX v-if="m.active" /><UserCheck v-else />
              </Button>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('org.remove')" @click="askRemove(m)"><UserMinus /></Button>
            </div>
          </li>
        </ul>
        <div v-if="isOwner" class="mt-6">
          <Button variant="outline" class="text-destructive" @click="deleteOrgOpen = true"><Trash2 /> {{ $t('org.deleteOrg') }}</Button>
        </div>
      </TabsContent>

      <TabsContent v-if="canInvite" value="invitations">
        <p v-if="org.invitations.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('org.noInvitations') }}</p>
        <ul v-else class="divide-y rounded-lg border">
          <li v-for="i in org.invitations" :key="i.id" class="flex items-center gap-3 px-3 py-2 text-sm">
            <span class="flex-1 truncate">{{ i.email }}</span>
            <span class="text-xs text-muted-foreground">{{ format.shortDate(i.createdAt) }}</span>
            <Badge variant="secondary">{{ $t(`org.roles.${i.role}`) }}</Badge>
            <Button size="icon" variant="ghost" class="size-7" :title="$t('org.withdraw')" @click="run(() => org.withdraw(i.id))"><X /></Button>
          </li>
        </ul>
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

      <TabsContent v-if="org.role && CAN.audit(org.role)" value="activity">
        <ul class="divide-y rounded-lg border">
          <li v-for="a in org.audit" :key="a.id" class="flex items-baseline gap-3 px-3 py-2 text-sm">
            <span class="w-40 shrink-0 text-xs text-muted-foreground">{{ format.shortDate(a.at) }} {{ format.time(a.at) }}</span>
            <span class="flex-1">{{ actionLabel(a.action) }}<span v-if="a.target" class="text-muted-foreground"> · {{ a.target.length === 36 ? memberName(a.target) : a.target }}</span></span>
            <span class="text-xs text-muted-foreground">{{ actor(a.actorId) }}</span>
          </li>
        </ul>
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
