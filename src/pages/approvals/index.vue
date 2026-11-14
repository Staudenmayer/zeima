<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Check, ClipboardCheck, Send, Undo2, X } from '@lucide/vue'
import { commands, type Absence, type TimesheetSubmission } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useSubmissionsStore } from '@/stores/submissions'
import { useWorkStore } from '@/stores/work'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.approvals' } })

const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()
const workspaces = useWorkspaceStore()
const submissions = useSubmissionsStore()
const work = useWorkStore()

/** Absence requests of other people waiting for a decision. */
const absenceRequests = ref<Absence[]>([])
const absenceRange = (a: Absence) => (a.startDate === a.endDate ? day(a.startDate) : `${day(a.startDate)} – ${day(a.endDate)}`)
const typeName = (a: Absence) => {
  const type = work.absenceTypesById.get(a.typeId)
  return type ? work.absenceTypeName(type) : ''
}
async function decideAbsence(a: Absence, approve: boolean, why = '') {
  await run(async () => {
    await call(commands.absenceDecide(a.id, approve, why))
    absenceRequests.value = await call(commands.absenceRequests())
  })
}

const me = computed(() => account.status.user?.id ?? null)
const mine = computed(() => submissions.all.filter((s) => s.userId === me.value))
const toApprove = computed(() => submissions.all.filter((s) => s.userId !== me.value && s.status === 'submitted'))
const decided = computed(() => submissions.all.filter((s) => s.userId !== me.value && s.status !== 'submitted'))

const nameOf = (id: string) => org.nameOf(id)
const day = (iso: string) => format.shortDate(Date.parse(`${iso}T12:00:00`))
const period = (s: TimesheetSubmission) => `${day(s.periodStart)} – ${day(s.periodEnd)}`

async function load() {
  if (!workspaces.inOrg) return
  try {
    await submissions.load()
    absenceRequests.value = await call(commands.absenceRequests())
    if (org.members.length === 0) await org.load().catch(() => {})
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch(() => workspaces.active?.id, load)

async function run(action: () => Promise<unknown>, done?: string) {
  try {
    await action()
    if (done) toast.success(done)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

// --------------------------------------------------------------- submitting
/** Monday of the week `offset` weeks from this one, and its Sunday, as `YYYY-MM-DD`. */
function week(offset: number): [string, string] {
  const d = new Date()
  d.setHours(12, 0, 0, 0)
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7) + offset * 7)
  const e = new Date(d)
  e.setDate(e.getDate() + 6)
  const iso = (x: Date) => `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, '0')}-${String(x.getDate()).padStart(2, '0')}`
  return [iso(d), iso(e)]
}
const submitOpen = ref(false)
const from = ref('')
const to = ref('')
const note = ref('')
function openSubmit() {
  ;[from.value, to.value] = week(-1)
  note.value = ''
  submitOpen.value = true
}
function preset(offset: number) {
  ;[from.value, to.value] = week(offset)
}
async function submit() {
  await run(async () => {
    await submissions.submit(from.value, to.value, note.value)
    submitOpen.value = false
  }, t('approvals.submitted'))
}

// ----------------------------------------------------------------- deciding
const deciding = ref<TimesheetSubmission | Absence | null>(null)
const decisionNote = ref('')
function askReject(s: TimesheetSubmission | Absence) {
  deciding.value = s
  decisionNote.value = ''
}
async function reject() {
  const s = deciding.value
  if (!s) return
  await run(async () => {
    if ('periodStart' in s) await submissions.decide(s.id, false, decisionNote.value)
    else await decideAbsence(s, false, decisionNote.value)
    deciding.value = null
  })
}

const statusVariant = (s: TimesheetSubmission) => (s.status === 'approved' ? 'default' : s.status === 'rejected' ? 'destructive' : 'secondary')
const { t } = useI18n()
</script>

<template>
  <PageHeader :title="$t('nav.approvals')">
    <Button v-if="workspaces.inOrg" @click="openSubmit"><Send /> {{ $t('approvals.submit') }}</Button>
  </PageHeader>

  <EmptyState v-if="!workspaces.inOrg" :icon="ClipboardCheck" :text="$t('approvals.personal')" />

  <div v-else class="grid gap-8">
    <section v-if="absenceRequests.length" class="grid gap-2">
      <h2 class="text-sm font-medium">{{ $t('approvals.absenceRequests') }}</h2>
      <ul class="divide-y rounded-lg border">
        <li v-for="a in absenceRequests" :key="a.id" class="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
          <div class="min-w-0 flex-1">
            <p class="font-medium">{{ nameOf(a.userId) || $t('approvals.someone') }} · {{ absenceRange(a) }}</p>
            <p class="truncate text-xs text-muted-foreground">{{ typeName(a) }}<span v-if="a.note"> · {{ a.note }}</span></p>
          </div>
          <Button size="sm" @click="decideAbsence(a, true)"><Check /> {{ $t('approvals.approve') }}</Button>
          <Button size="sm" variant="outline" @click="askReject(a)"><X /> {{ $t('approvals.reject') }}</Button>
        </li>
      </ul>
    </section>

    <section v-if="toApprove.length" class="grid gap-2">
      <h2 class="text-sm font-medium">{{ $t('approvals.toApprove') }}</h2>
      <ul class="divide-y rounded-lg border">
        <li v-for="s in toApprove" :key="s.id" class="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
          <div class="min-w-0 flex-1">
            <p class="font-medium">{{ nameOf(s.userId) || $t('approvals.someone') }} · {{ period(s) }}</p>
            <p v-if="s.note" class="truncate text-xs text-muted-foreground">{{ s.note }}</p>
          </div>
          <Button size="sm" @click="run(() => submissions.decide(s.id, true, ''), $t('approvals.approvedToast'))"><Check /> {{ $t('approvals.approve') }}</Button>
          <Button size="sm" variant="outline" @click="askReject(s)"><X /> {{ $t('approvals.reject') }}</Button>
        </li>
      </ul>
      <p class="text-xs text-muted-foreground">{{ $t('approvals.approveHint') }}</p>
    </section>

    <section class="grid gap-2">
      <h2 class="text-sm font-medium">{{ $t('approvals.mine') }}</h2>
      <p v-if="mine.length === 0" class="py-4 text-center text-sm text-muted-foreground">{{ $t('approvals.noneYet') }}</p>
      <ul v-else class="divide-y rounded-lg border">
        <li v-for="s in mine" :key="s.id" class="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
          <div class="min-w-0 flex-1">
            <p class="font-medium">{{ period(s) }}</p>
            <p v-if="s.status === 'rejected' && s.decisionNote" class="text-xs text-muted-foreground">{{ $t('approvals.reason', { reason: s.decisionNote }) }}</p>
            <p v-else-if="s.note" class="truncate text-xs text-muted-foreground">{{ s.note }}</p>
          </div>
          <Badge :variant="statusVariant(s)">{{ $t(`approvals.status.${s.status}`) }}</Badge>
          <Button v-if="s.status === 'submitted'" size="sm" variant="ghost" @click="run(() => submissions.withdraw(s.id))"><Undo2 /> {{ $t('approvals.withdraw') }}</Button>
          <Button v-else-if="s.status === 'rejected'" size="sm" variant="outline" @click="run(() => submissions.submit(s.periodStart, s.periodEnd, s.note), $t('approvals.submitted'))">
            <Send /> {{ $t('approvals.again') }}
          </Button>
        </li>
      </ul>
    </section>

    <section v-if="decided.length" class="grid gap-2">
      <h2 class="text-sm font-medium">{{ $t('approvals.decided') }}</h2>
      <ul class="divide-y rounded-lg border">
        <li v-for="s in decided" :key="s.id" class="flex items-center gap-3 px-3 py-2 text-sm">
          <span class="flex-1">{{ nameOf(s.userId) || $t('approvals.someone') }} · {{ period(s) }}</span>
          <Badge :variant="statusVariant(s)">{{ $t(`approvals.status.${s.status}`) }}</Badge>
          <Button v-if="s.status === 'approved'" size="sm" variant="ghost" @click="run(() => submissions.reopen(s.id, ''), $t('approvals.reopenedToast'))">
            <Undo2 /> {{ $t('approvals.reopen') }}
          </Button>
        </li>
      </ul>
    </section>
  </div>

  <Dialog v-model:open="submitOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('approvals.submit') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="submit">
        <div class="flex gap-2">
          <Button type="button" size="sm" variant="outline" @click="preset(-1)">{{ $t('approvals.lastWeek') }}</Button>
          <Button type="button" size="sm" variant="outline" @click="preset(0)">{{ $t('approvals.thisWeek') }}</Button>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2"><Label for="sub-from">{{ $t('approvals.from') }}</Label><Input id="sub-from" v-model="from" type="date" required /></div>
          <div class="grid gap-2"><Label for="sub-to">{{ $t('approvals.to') }}</Label><Input id="sub-to" v-model="to" type="date" required /></div>
        </div>
        <div class="grid gap-2"><Label for="sub-note">{{ $t('approvals.note') }}</Label><Input id="sub-note" v-model="note" maxlength="500" /></div>
        <p class="text-xs text-muted-foreground">{{ $t('approvals.lockHint') }}</p>
        <DialogFooter>
          <Button type="button" variant="outline" @click="submitOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('approvals.submit') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <Dialog :open="deciding !== null" @update:open="(o) => !o && (deciding = null)">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('approvals.reject') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="reject">
        <div class="grid gap-2"><Label for="reject-note">{{ $t('approvals.rejectReason') }}</Label><Input id="reject-note" v-model="decisionNote" maxlength="500" autofocus /></div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="deciding = null">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('approvals.reject') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
