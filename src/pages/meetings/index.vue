<script setup lang="ts">
import TableToolbar from '@/components/time/TableToolbar.vue'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import { passes, useTableSearch } from '@/composables/useTableSearch'
import UserAvatar from '@/components/time/UserAvatar.vue'
import MeetingPeople from '@/components/time/MeetingPeople.vue'
import MeetingDetailsDialog from '@/components/time/MeetingDetailsDialog.vue'
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Ban, CalendarPlus, CalendarX2, DoorOpen, Download, ExternalLink, MapPin, Pencil, Plus } from '@lucide/vue'
import { commands, type AddonTimelineItem, type Meeting, type MeetingOccurrence, type RsvpStatus } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import MeetingDialog from '@/components/time/MeetingDialog.vue'
import RoomsDialog from '@/components/time/RoomsDialog.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, startOfDay, toDateInput, toTimeInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'

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

const organizers = ref<(string | null)[]>([])
const projectIds = ref<(string | null)[]>([])
const organizerOptions = computed<FilterOption[]>(() => [...new Set(items.value.map((o) => o.meeting.userId))].map((id) => ({ value: id, label: org.nameOf(id) || '–', person: true })))
const projectOptions = computed<FilterOption[]>(() => [...new Set(items.value.flatMap((o) => (o.meeting.projectId ? [o.meeting.projectId] : [])))].map((id) => ({ value: id, label: projects.byId.get(id)?.name ?? id })))
const { search, filtered, searching } = useTableSearch(
  items,
  (o) => [o.meeting.title, o.meeting.location, o.meeting.onlineUrl, o.meeting.projectId ? projects.byId.get(o.meeting.projectId)?.name : '', org.nameOf(o.meeting.userId), ...o.meeting.attendeeIds.map((id) => org.nameOf(id))],
  (o) => passes(organizers.value, o.meeting.userId) && passes(projectIds.value, o.meeting.projectId),
)
const narrowed = computed(() => searching.value || organizers.value.length > 0 || projectIds.value.length > 0)
const clearFilters = () => {
  search.value = ''
  organizers.value = []
  projectIds.value = []
}

/** Upcoming first; meetings that ended earlier today stay until tomorrow. */
const days = computed(() => {
  const groups = new Map<string, MeetingOccurrence[]>()
  for (const o of filtered.value) {
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

async function run(action: () => Promise<unknown>, done?: string) {
  try {
    await action()
    if (done) toast.success(done)
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
// A repeating meeting is answered for all its dates, or for one date only (that date's answer counts instead).
const dateOnly = ref(new Map<string, boolean>())
const rowKey = (o: MeetingOccurrence) => `${o.meeting.id}/${o.startAt}`
const isDateOnly = (o: MeetingOccurrence) => dateOnly.value.get(rowKey(o)) ?? o.myDateAnswer
const respond = (o: MeetingOccurrence, status: RsvpStatus) =>
  run(() => call(commands.meetingRespond(o.meeting.id, status, o.meeting.rrule && isDateOnly(o) ? o.startAt : null)))
function chooseScope(o: MeetingOccurrence, only: boolean) {
  dateOnly.value = new Map(dateOnly.value).set(rowKey(o), only)
  // Back to all dates: the answer for this date is taken back, so the series answer counts again.
  if (!only && o.myDateAnswer) void run(() => call(commands.meetingRespondClear(o.meeting.id, o.startAt)))
}

// A row opens the meeting: when, where, who takes part and what each answered.
const detailsOpen = ref(false)
const detailsItem = ref<AddonTimelineItem | null>(null)
function openDetails(o: MeetingOccurrence) {
  detailsItem.value = {
    addonId: 'zeima.meetings',
    id: `${o.meeting.id}@${o.startAt}`,
    title: o.meeting.title,
    startAt: o.startAt,
    endAt: o.endAt,
    allDay: false,
    detail: o.meeting.location,
    color: null,
    activityId: o.meeting.activityId,
    projectId: o.meeting.projectId,
    tagIds: o.meeting.tagIds,
    billable: o.meeting.billable,
    costCenterId: o.meeting.costCenterId,
    note: o.meeting.title,
  }
  detailsOpen.value = true
}

/** Whether the opened meeting is the person's own (and not called off), so they can change it. */
const detailsMine = computed(() => {
  const id = detailsItem.value?.id.split('@')[0]
  const m = items.value.find((o) => o.meeting.id === id)?.meeting
  return !!m && m.userId === me.value && !m.cancelled
})
const dialogOpen = ref(false)
const roomsOpen = ref(false)
const editing = ref<Meeting | null>(null)
function openDialog(m: Meeting | null) {
  editing.value = m
  dialogOpen.value = true
}
const cancelling = ref<Meeting | null>(null)
const cancelOpen = ref(false)

// Sorting keeps the days; the meetings of each day change order.
const { sort, apply: sortOccurrences } = useSort<MeetingOccurrence>(null, {
  time: (o) => o.startAt,
  title: (o) => o.meeting.title,
  location: (o) => o.meeting.location || (o.meeting.onlineUrl ? 'online' : ''),
  project: (o) => (o.meeting.projectId && projects.byId.get(o.meeting.projectId)?.name) || '',
  attendees: (o) => o.meeting.attendeeIds.length,
  answer: (o) => o.myStatus ?? '',
})
</script>

<template>
  <PageHeader :title="$t('nav.meetings')">
    <Button v-if="workspaces.inOrg && workspaces.can('policy.write')" variant="outline" @click="roomsOpen = true"><DoorOpen /> {{ $t('meetings.rooms') }}</Button>
    <Button v-if="workspaces.inOrg" @click="openDialog(null)"><Plus /> {{ $t('meetings.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="!workspaces.inOrg" :icon="CalendarPlus" :text="$t('meetings.personal')" />
  <EmptyState v-else-if="items.length === 0" :icon="CalendarX2" :text="$t('meetings.empty')">
    <Button variant="outline" @click="openDialog(null)"><Plus /> {{ $t('meetings.new') }}</Button>
  </EmptyState>

  <div v-else class="grid gap-3">
    <TableToolbar v-model="search" class="!mb-0" :active="narrowed" @clear="clearFilters">
      <MultiFilter v-model="organizers" :label="$t('meetings.organizer')" :options="organizerOptions" />
      <MultiFilter v-if="projectOptions.length" v-model="projectIds" :label="$t('meetings.projectColumn')" :options="projectOptions" />
    </TableToolbar>
    <p v-if="filtered.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
    <div v-else class="overflow-x-auto rounded-lg border">
      <table class="w-full text-sm">
        <thead class="text-left text-xs text-muted-foreground">
          <tr class="border-b">
            <SortHead :sort="sort" column="time" class="w-28 px-3 py-2 font-medium">{{ $t('timesheet.time') }}</SortHead>
            <SortHead :sort="sort" column="title" class="px-3 py-2 font-medium">{{ $t('meetings.title') }}</SortHead>
            <SortHead :sort="sort" column="location" class="px-3 py-2 font-medium">{{ $t('meetings.location') }}</SortHead>
            <SortHead :sort="sort" column="project" class="px-3 py-2 font-medium">{{ $t('meetings.projectColumn') }}</SortHead>
            <SortHead :sort="sort" column="attendees" class="px-3 py-2 font-medium">{{ $t('meetings.attendees') }}</SortHead>
            <SortHead :sort="sort" column="answer" class="px-3 py-2 font-medium">{{ $t('meetings.answer') }}</SortHead>
            <th class="w-28 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
          </tr>
        </thead>
        <tbody v-for="d in days" :key="d.date">
          <tr class="border-b bg-muted/40">
            <th colspan="7" class="px-3 py-1.5 text-left font-medium">{{ dayLabel(d.date) }}</th>
          </tr>
          <tr
            v-for="o in sortOccurrences(d.list)"
            :key="`${o.meeting.id}@${o.startAt}`"
            class="group border-b last:border-b-0 hover:bg-muted/30"
            :class="[o.meeting.cancelled ? 'opacity-60' : '', 'cursor-pointer']"
            @click="openDetails(o)"
          >
            <td class="whitespace-nowrap px-3 py-1.5 tabular-nums text-muted-foreground">{{ when(o) }}</td>
            <td class="max-w-72 px-3 py-1.5">
              <div class="flex items-center gap-2">
                <span class="truncate font-medium" :class="o.meeting.cancelled ? 'line-through' : ''" :title="o.meeting.note || undefined">{{ o.meeting.title }}</span>
                <Badge v-if="o.meeting.cancelled" variant="destructive">{{ $t('meetings.cancelled') }}</Badge>
                <Badge v-else-if="o.meeting.rrule" variant="outline">{{ $t('meetings.repeating') }}</Badge>
              </div>
              <p v-if="organizer(o.meeting)" class="flex items-center gap-1 truncate text-xs text-muted-foreground"><UserAvatar :user-id="o.meeting.userId" size="xs" />{{ $t('meetings.by', { name: organizer(o.meeting) }) }}</p>
              <p v-if="o.meeting.note" class="truncate text-xs text-muted-foreground" :title="o.meeting.note">{{ o.meeting.note }}</p>
            </td>
            <td class="px-3 py-1.5 text-muted-foreground" @click.stop>
              <span v-if="o.meeting.location" class="flex items-center gap-1"><MapPin class="size-3 shrink-0" /> {{ o.meeting.location }}</span>
              <button v-if="o.meeting.onlineUrl" type="button" class="flex items-center gap-1 whitespace-nowrap hover:underline" @click="openUrl(o.meeting.onlineUrl)"><ExternalLink class="size-3 shrink-0" /> {{ $t('meetings.join') }}</button>
              <span v-if="!o.meeting.location && !o.meeting.onlineUrl">–</span>
            </td>
            <td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">{{ (o.meeting.projectId && projects.byId.get(o.meeting.projectId)?.name) || '–' }}</td>
            <td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">
              <template v-if="o.meeting.attendeeIds.length">
                <MeetingPeople :occurrence="o" />
                <p class="mt-0.5 text-xs">{{ $t('meetings.counts', counts(o)) }}</p>
              </template>
              <span v-else>–</span>
            </td>
            <td class="whitespace-nowrap px-3 py-1.5" @click.stop>
              <div v-if="o.meeting.userId !== me && !o.meeting.cancelled" class="flex gap-1">
                <Button size="sm" class="h-7" :variant="o.myStatus === 'accepted' ? 'default' : 'outline'" @click="respond(o, 'accepted')">{{ $t('meetings.accept') }}</Button>
                <Button size="sm" class="h-7" :variant="o.myStatus === 'tentative' ? 'default' : 'outline'" @click="respond(o, 'tentative')">{{ $t('meetings.maybe') }}</Button>
                <Button size="sm" class="h-7" :variant="o.myStatus === 'declined' ? 'destructive' : 'outline'" @click="respond(o, 'declined')">{{ $t('meetings.decline') }}</Button>
              </div>
              <div v-if="o.meeting.userId !== me && !o.meeting.cancelled && o.meeting.rrule" class="mt-1 flex items-center gap-1 text-xs">
                <button type="button" class="rounded px-1.5 py-0.5" :class="!isDateOnly(o) ? 'bg-muted font-medium' : 'text-muted-foreground hover:bg-muted/60'" @click="chooseScope(o, false)">{{ $t('meetings.allDates') }}</button>
                <button type="button" class="rounded px-1.5 py-0.5" :class="isDateOnly(o) ? 'bg-muted font-medium' : 'text-muted-foreground hover:bg-muted/60'" @click="chooseScope(o, true)">{{ $t('meetings.thisDate') }}</button>
                <span v-if="o.myDateAnswer && o.seriesStatus" class="text-muted-foreground">· {{ $t('meetings.seriesAnswer', { answer: $t(`meetings.${o.seriesStatus === 'accepted' ? 'accept' : o.seriesStatus === 'tentative' ? 'maybe' : 'decline'}`) }) }}</span>
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
  <MeetingDetailsDialog v-model:open="detailsOpen" :item="detailsItem" :can-track="false" :can-edit="detailsItem !== null && detailsMine" @edit="openDialog" @answered="load" />
  <RoomsDialog v-model:open="roomsOpen" />
  <ConfirmDialog
    v-model:open="cancelOpen"
    :title="$t('meetings.cancelConfirm', { title: cancelling?.title ?? '' })"
    :description="$t('meetings.cancelHint')"
    @confirm="cancelling && run(() => call(commands.meetingCancel(cancelling!.id)))"
  />
</template>
