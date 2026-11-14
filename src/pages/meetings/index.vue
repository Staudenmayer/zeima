<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Ban, CalendarPlus, CalendarX2, Download, ExternalLink, MapPin, Pencil, Plus } from '@lucide/vue'
import { commands, type Meeting, type MeetingOccurrence, type RsvpStatus } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import MeetingDialog from '@/components/time/MeetingDialog.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, startOfDay, toDateInput, toTimeInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.meetings' } })

const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()
const projects = useProjectsStore()
const workspaces = useWorkspaceStore()

const items = ref<MeetingOccurrence[]>([])
const me = computed(() => account.status.user?.id ?? '')
const HORIZON_DAYS = 45

async function load() {
  if (!workspaces.inOrg) return
  try {
    const today = Date.now()
    if (org.people.length === 0 && workspaces.active?.orgId) await org.loadGlance(workspaces.active.orgId).catch(() => {})
    items.value = await call(commands.meetingsList(toDateInput(addDays(startOfDay(today), -1)), toDateInput(addDays(startOfDay(today), HORIZON_DAYS))))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch(() => workspaces.active?.id, load)

/** Upcoming first; meetings that ended earlier today stay until tomorrow. */
const days = computed(() => {
  const groups = new Map<string, MeetingOccurrence[]>()
  for (const o of items.value) {
    const key = toDateInput(o.startAt)
    groups.set(key, [...(groups.get(key) ?? []), o])
  }
  return [...groups.entries()].map(([date, list]) => ({ date, list }))
})
const dayLabel = (date: string) => format.day(Date.parse(`${date}T12:00:00`))
const when = (o: MeetingOccurrence) => `${toTimeInput(o.startAt)}–${toTimeInput(o.endAt)}`
const organizer = (m: Meeting) => (m.userId === me.value ? '' : org.nameOf(m.userId))
const counts = (o: MeetingOccurrence) => {
  const by = (s: RsvpStatus) => o.responses.filter((r) => r.status === s).length
  return { accepted: by('accepted'), tentative: by('tentative'), declined: by('declined'), waiting: o.meeting.attendeeIds.length - o.responses.length }
}
const names = (o: MeetingOccurrence, s: RsvpStatus) => o.responses.filter((r) => r.status === s).map((r) => org.nameOf(r.userId) || '·').join(', ')

async function run(action: () => Promise<unknown>, done?: string) {
  try {
    await action()
    if (done) toast.success(done)
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const respond = (o: MeetingOccurrence, status: RsvpStatus) => run(() => call(commands.meetingRespond(o.meeting.id, status)))

const dialogOpen = ref(false)
const editing = ref<Meeting | null>(null)
function openDialog(m: Meeting | null) {
  editing.value = m
  dialogOpen.value = true
}
const cancelling = ref<Meeting | null>(null)
const cancelOpen = ref(false)
</script>

<template>
  <PageHeader :title="$t('nav.meetings')">
    <Button v-if="workspaces.inOrg" @click="openDialog(null)"><Plus /> {{ $t('meetings.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="!workspaces.inOrg" :icon="CalendarPlus" :text="$t('meetings.personal')" />
  <EmptyState v-else-if="items.length === 0" :icon="CalendarX2" :text="$t('meetings.empty')">
    <Button variant="outline" @click="openDialog(null)"><Plus /> {{ $t('meetings.new') }}</Button>
  </EmptyState>

  <div v-else class="grid gap-6">
    <section v-for="d in days" :key="d.date" class="grid gap-2">
      <h2 class="text-sm font-medium">{{ dayLabel(d.date) }}</h2>
      <ul class="grid gap-2">
        <li v-for="o in d.list" :key="`${o.meeting.id}@${o.startAt}`" class="rounded-lg border p-3 text-sm" :class="o.meeting.cancelled ? 'opacity-60' : ''">
          <div class="flex flex-wrap items-center gap-3">
            <span class="w-28 tabular-nums text-muted-foreground">{{ when(o) }}</span>
            <span class="flex-1 font-medium" :class="o.meeting.cancelled ? 'line-through' : ''">{{ o.meeting.title }}</span>
            <Badge v-if="o.meeting.cancelled" variant="destructive">{{ $t('meetings.cancelled') }}</Badge>
            <Badge v-else-if="o.meeting.rrule" variant="outline">{{ $t('meetings.repeating') }}</Badge>
            <span v-if="organizer(o.meeting)" class="text-xs text-muted-foreground">{{ $t('meetings.by', { name: organizer(o.meeting) }) }}</span>
          </div>
          <div class="mt-1 flex flex-wrap items-center gap-3 pl-[7.75rem] text-xs text-muted-foreground">
            <span v-if="o.meeting.location" class="flex items-center gap-1"><MapPin class="size-3" /> {{ o.meeting.location }}</span>
            <button v-if="o.meeting.onlineUrl" type="button" class="flex items-center gap-1 hover:underline" @click="openUrl(o.meeting.onlineUrl)"><ExternalLink class="size-3" /> {{ $t('meetings.join') }}</button>
            <span v-if="o.meeting.projectId && projects.byId.get(o.meeting.projectId)">{{ projects.byId.get(o.meeting.projectId)?.name }}</span>
            <span v-if="o.meeting.attendeeIds.length" :title="[names(o, 'accepted') && `✓ ${names(o, 'accepted')}`, names(o, 'tentative') && `? ${names(o, 'tentative')}`, names(o, 'declined') && `✗ ${names(o, 'declined')}`].filter(Boolean).join('\n')">
              {{ $t('meetings.counts', counts(o)) }}
            </span>
          </div>
          <p v-if="o.meeting.note" class="mt-1 whitespace-pre-line pl-[7.75rem] text-xs text-muted-foreground">{{ o.meeting.note }}</p>
          <div v-if="!o.meeting.cancelled" class="mt-2 flex flex-wrap items-center gap-2 pl-[7.75rem]">
            <template v-if="o.meeting.userId !== me">
              <Button size="sm" :variant="o.myStatus === 'accepted' ? 'default' : 'outline'" @click="respond(o, 'accepted')">{{ $t('meetings.accept') }}</Button>
              <Button size="sm" :variant="o.myStatus === 'tentative' ? 'default' : 'outline'" @click="respond(o, 'tentative')">{{ $t('meetings.maybe') }}</Button>
              <Button size="sm" :variant="o.myStatus === 'declined' ? 'destructive' : 'outline'" @click="respond(o, 'declined')">{{ $t('meetings.decline') }}</Button>
            </template>
            <template v-else>
              <Button size="sm" variant="ghost" @click="openDialog(o.meeting)"><Pencil /> {{ $t('common.edit') }}</Button>
              <Button size="sm" variant="ghost" @click="((cancelling = o.meeting), (cancelOpen = true))"><Ban /> {{ $t('meetings.cancel') }}</Button>
            </template>
            <Button size="sm" variant="ghost" @click="run(() => call(commands.meetingExportIcs(o.meeting.id)))"><Download /> {{ $t('meetings.export') }}</Button>
          </div>
        </li>
      </ul>
    </section>
    <p class="text-xs text-muted-foreground">{{ $t('meetings.hint') }}</p>
  </div>

  <MeetingDialog v-model:open="dialogOpen" :meeting="editing" @saved="load" />
  <ConfirmDialog
    v-model:open="cancelOpen"
    :title="$t('meetings.cancelConfirm', { title: cancelling?.title ?? '' })"
    :description="$t('meetings.cancelHint')"
    @confirm="cancelling && run(() => call(commands.meetingCancel(cancelling!.id)))"
  />
</template>
