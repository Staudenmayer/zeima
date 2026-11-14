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

  <div v-else class="grid gap-3">
    <div class="overflow-x-auto rounded-lg border">
      <table class="w-full text-sm">
        <thead class="text-left text-xs text-muted-foreground">
          <tr class="border-b">
            <th class="w-28 px-3 py-2 font-medium">{{ $t('timesheet.time') }}</th>
            <th class="px-3 py-2 font-medium">{{ $t('meetings.title') }}</th>
            <th class="px-3 py-2 font-medium">{{ $t('meetings.location') }}</th>
            <th class="px-3 py-2 font-medium">{{ $t('meetings.projectColumn') }}</th>
            <th class="px-3 py-2 font-medium">{{ $t('meetings.attendees') }}</th>
            <th class="px-3 py-2 font-medium">{{ $t('meetings.answer') }}</th>
            <th class="w-28 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
          </tr>
        </thead>
        <tbody v-for="d in days" :key="d.date">
          <tr class="border-b bg-muted/40">
            <th colspan="7" class="px-3 py-1.5 text-left font-medium">{{ dayLabel(d.date) }}</th>
          </tr>
          <tr
            v-for="o in d.list"
            :key="`${o.meeting.id}@${o.startAt}`"
            class="group border-b last:border-b-0 hover:bg-muted/30"
            :class="[o.meeting.cancelled ? 'opacity-60' : '', o.meeting.userId === me && !o.meeting.cancelled ? 'cursor-pointer' : '']"
            @click="o.meeting.userId === me && !o.meeting.cancelled && openDialog(o.meeting)"
          >
            <td class="whitespace-nowrap px-3 py-1.5 tabular-nums text-muted-foreground">{{ when(o) }}</td>
            <td class="max-w-72 px-3 py-1.5">
              <div class="flex items-center gap-2">
                <span class="truncate font-medium" :class="o.meeting.cancelled ? 'line-through' : ''" :title="o.meeting.note || undefined">{{ o.meeting.title }}</span>
                <Badge v-if="o.meeting.cancelled" variant="destructive">{{ $t('meetings.cancelled') }}</Badge>
                <Badge v-else-if="o.meeting.rrule" variant="outline">{{ $t('meetings.repeating') }}</Badge>
              </div>
              <p v-if="organizer(o.meeting)" class="truncate text-xs text-muted-foreground">{{ $t('meetings.by', { name: organizer(o.meeting) }) }}</p>
              <p v-if="o.meeting.note" class="truncate text-xs text-muted-foreground" :title="o.meeting.note">{{ o.meeting.note }}</p>
            </td>
            <td class="px-3 py-1.5 text-muted-foreground" @click.stop>
              <span v-if="o.meeting.location" class="flex items-center gap-1"><MapPin class="size-3 shrink-0" /> {{ o.meeting.location }}</span>
              <button v-if="o.meeting.onlineUrl" type="button" class="flex items-center gap-1 whitespace-nowrap hover:underline" @click="openUrl(o.meeting.onlineUrl)"><ExternalLink class="size-3 shrink-0" /> {{ $t('meetings.join') }}</button>
              <span v-if="!o.meeting.location && !o.meeting.onlineUrl">–</span>
            </td>
            <td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">{{ (o.meeting.projectId && projects.byId.get(o.meeting.projectId)?.name) || '–' }}</td>
            <td
              class="whitespace-nowrap px-3 py-1.5 text-muted-foreground"
              :title="[names(o, 'accepted') && `✓ ${names(o, 'accepted')}`, names(o, 'tentative') && `? ${names(o, 'tentative')}`, names(o, 'declined') && `✗ ${names(o, 'declined')}`].filter(Boolean).join('\n')"
            >{{ o.meeting.attendeeIds.length ? $t('meetings.counts', counts(o)) : '–' }}</td>
            <td class="whitespace-nowrap px-3 py-1.5" @click.stop>
              <div v-if="o.meeting.userId !== me && !o.meeting.cancelled" class="flex gap-1">
                <Button size="sm" class="h-7" :variant="o.myStatus === 'accepted' ? 'default' : 'outline'" @click="respond(o, 'accepted')">{{ $t('meetings.accept') }}</Button>
                <Button size="sm" class="h-7" :variant="o.myStatus === 'tentative' ? 'default' : 'outline'" @click="respond(o, 'tentative')">{{ $t('meetings.maybe') }}</Button>
                <Button size="sm" class="h-7" :variant="o.myStatus === 'declined' ? 'destructive' : 'outline'" @click="respond(o, 'declined')">{{ $t('meetings.decline') }}</Button>
              </div>
              <span v-else class="text-muted-foreground">–</span>
            </td>
            <td class="px-1 py-0.5" @click.stop>
              <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                <template v-if="o.meeting.userId === me && !o.meeting.cancelled">
                  <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="openDialog(o.meeting)"><Pencil /></Button>
                  <Button size="icon" variant="ghost" class="size-7" :title="$t('meetings.cancel')" @click="((cancelling = o.meeting), (cancelOpen = true))"><Ban /></Button>
                </template>
                <Button size="icon" variant="ghost" class="size-7" :title="$t('meetings.export')" @click="run(() => call(commands.meetingExportIcs(o.meeting.id)))"><Download /></Button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
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
