<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { useStorage } from '@vueuse/core'
import { CalendarDays, ChevronDown, ChevronLeft, ChevronRight, Clock, Copy, List, Pencil, Play, Plus, Scissors, Trash2, X } from '@lucide/vue'
import { commands, type AddonTimelineItem, type EntryPatch, type TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import TimeInput from '@/components/time/TimeInput.vue'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import ActivityLabel from '@/components/time/ActivityLabel.vue'
import DayTimeline from '@/components/time/DayTimeline.vue'
import EntryCalendar from '@/components/time/EntryCalendar.vue'
import EntryDialog from '@/components/time/EntryDialog.vue'
import EntryPopover from '@/components/time/EntryPopover.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import TagList from '@/components/time/TagList.vue'
import { useEntries } from '@/composables/useEntries'
import type { EntryDraft } from '@/composables/useEntryForm'
import { useTimeline } from '@/composables/useTimeline'
import { useWorkDays } from '@/composables/useWorkDays'
import DayStatus from '@/components/time/DayStatus.vue'
import { useFormat } from '@/composables/useFormat'
import type { Anchor } from '@/lib/anchor'
import { call, errorMessage } from '@/lib/ipc'
import { HOUR, addDays, dayRange, daysBetween, fromInputs, startOfDay, toDateInput, toTimeInput, weekDays } from '@/lib/time'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTagsStore } from '@/stores/tags'
import { useTimerStore } from '@/stores/timer'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.timesheet', fill: true } })

const { t } = useI18n()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const settings = useSettingsStore()
const tags = useTagsStore()
const timer = useTimerStore()
const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()
const workspaces = useWorkspaceStore()

// Remembered: day/week and calendar/table across restarts; the shown date
// only while the app runs (a restart starts on today again).
const mode = useStorage<'day' | 'week'>('timesheet.mode', 'day')
const view = useStorage<'table' | 'calendar'>('timesheet.viewMode', 'calendar')
const anchor = useStorage('timesheet.anchor', startOfDay(Date.now()), sessionStorage)

const days = computed(() => (mode.value === 'day' ? [dayRange(anchor.value)] : weekDays(anchor.value)))
const range = computed(() => ({ from: days.value[0].from, to: days.value[days.value.length - 1].to }))
const { entries, summaries } = useEntries(range, days)
const { byDate, holidayName } = useWorkDays(range)
const { items: timeline } = useTimeline(range)

// ---------------------------------------------------------------- filters
// Entries, meetings and addon events are filtered alike, in every view.
const KINDS = ['entries', 'events'] as const
const filter = ref({
  kinds: [] as (string | null)[],
  activityIds: [] as (string | null)[],
  projectIds: [] as (string | null)[],
  tagIds: [] as (string | null)[],
  billable: 'all' as 'all' | 'yes' | 'no',
})
const hasFilter = computed(() => {
  const f = filter.value
  return f.kinds.length + f.activityIds.length + f.projectIds.length + f.tagIds.length > 0 || f.billable !== 'all'
})
function clearFilter() {
  filter.value = { kinds: [], activityIds: [], projectIds: [], tagIds: [], billable: 'all' }
}
const kindOptions = computed<FilterOption[]>(() => KINDS.map((k) => ({ value: k, label: t(`timesheet.kinds.${k}`) })))
const activityOptions = computed<FilterOption[]>(() => [
  ...activities.all.map((a) => ({ value: a.id, label: a.name, color: a.color })),
  { value: null, label: t('activities.none') },
])
const projectOptions = computed<FilterOption[]>(() => [
  ...projects.all.map((p) => ({ value: p.id, label: p.alias ? `${p.name} (${p.alias})` : p.name, color: p.color })),
  { value: null, label: t('common.noProject') },
])
const tagOptions = computed<FilterOption[]>(() => [
  ...tags.all.map((tag) => ({ value: tag.id, label: tag.name, color: tag.color })),
  { value: null, label: t('reports.noTag') },
])
type Filterable = { activityId: string | null; projectId: string | null; tagIds: string[]; billable: boolean }
function matches(x: Filterable): boolean {
  const f = filter.value
  if (f.activityIds.length && !f.activityIds.includes(x.activityId)) return false
  if (f.projectIds.length && !f.projectIds.includes(x.projectId)) return false
  if (f.tagIds.length && !(x.tagIds.length ? x.tagIds.some((id) => f.tagIds.includes(id)) : f.tagIds.includes(null))) return false
  if (f.billable !== 'all' && x.billable !== (f.billable === 'yes')) return false
  return true
}
const showsKind = (kind: (typeof KINDS)[number]) => filter.value.kinds.length === 0 || filter.value.kinds.includes(kind)
const filterEntries = (list: TimeEntry[]) => (showsKind('entries') ? list.filter(matches) : [])
const shownEntries = computed(() => filterEntries(entries.value))
const shownTimeline = computed(() => (showsKind('events') ? timeline.value.filter(matches) : []))

// Owners and managers can lay other people's days next to their own (day view).
const canPickPeople = computed(() => workspaces.inOrg && (workspaces.role === 'owner' || workspaces.role === 'manager'))
watch(
  () => (canPickPeople.value ? workspaces.active?.orgId : null),
  (orgId) => {
    if (orgId && org.people.length === 0) void org.loadGlance(orgId).catch(() => {})
  },
  { immediate: true },
)
const candidates = computed(() =>
  canPickPeople.value
    ? org.people
        .filter((p) => p.id !== account.status.user?.id)
        .map((p) => ({ id: p.id, name: p.name || org.nameOf(p.id) || t('team.someone') }))
    : [],
)
/** Added people, kept while the page is open; unknown ones (another workspace) drop out. */
const peopleIds = ref<string[]>([])
const shownIds = computed(() => (canPickPeople.value ? peopleIds.value.filter((id) => candidates.value.some((c) => c.id === id)) : []))
const otherDays = ref<{ id: string; name: string; entries: TimeEntry[] }[]>([])
const shownOthers = computed(() => otherDays.value.map((o) => ({ ...o, entries: filterEntries(o.entries) })))
let loadToken = 0
async function loadOthers() {
  const token = ++loadToken
  const ids = mode.value === 'day' && view.value === 'calendar' ? shownIds.value : []
  const rows = await Promise.all(
    ids.map(async (id) => ({
      id,
      name: candidates.value.find((c) => c.id === id)?.name ?? '',
      entries: await call(commands.teamEntries(id, range.value)).catch(() => [] as TimeEntry[]),
    })),
  )
  if (token === loadToken) otherDays.value = rows
}
watch([shownIds, range, mode, view], () => void loadOthers(), { immediate: true })

const title = computed(() =>
  mode.value === 'day'
    ? format.day(anchor.value)
    : `${format.shortDate(range.value.from)} – ${format.longDate(days.value[6].from)}`,
)

function move(direction: -1 | 1) {
  anchor.value = addDays(anchor.value, direction * (mode.value === 'day' ? 1 : 7))
}

/** Tracked time of day `i`: the totals of the server, or of what the filter shows. */
function totalOf(i: number): number {
  if (!hasFilter.value) return summaries.value[i]?.totalMs ?? 0
  const day = days.value[i]!
  return shownEntries.value.filter((e) => e.startAt >= day.from && e.startAt < day.to).reduce((n, e) => n + durationMs(e), 0)
}

/** Entries grouped by the day they start, newest day first. */
const groups = computed(() =>
  days.value
    .map((day, i) => ({
      day,
      work: byDate(toDateInput(day.from)),
      totalMs: totalOf(i),
      entries: shownEntries.value.filter((e) => e.startAt >= day.from && e.startAt < day.to).reverse(),
      // Meetings and addon events of the day (as in the calendar), to track from here.
      events: shownTimeline.value.filter((i) => !i.allDay && i.startAt >= day.from && i.startAt < day.to),
    }))
    .filter((g) => mode.value === 'day' || g.entries.length > 0 || g.events.length > 0)
    .reverse(),
)

function durationMs(e: TimeEntry) {
  return (e.endAt ?? timer.now) - e.startAt
}

// Dialogs
const entryDialogOpen = ref(false)
const editing = ref<TimeEntry | null>(null)
const newEntryDay = ref<number>(Date.now())
const draft = ref<EntryDraft | null>(null)

function openNew(day?: number) {
  editing.value = null
  draft.value = null
  newEntryDay.value = day ?? (mode.value === 'day' ? anchor.value : Date.now())
  entryDialogOpen.value = true
}

/** A new one-hour entry starting at `at` (double-click in the day timeline). */
function createAt(at: number) {
  editing.value = null
  draft.value = { startAt: at, endAt: at + HOUR, activityId: null, projectId: null, tagIds: [], note: '' }
  entryDialogOpen.value = true
}

/** "Track this event": a new entry with the item's times and values. */
function track(item: AddonTimelineItem) {
  editing.value = null
  draft.value = {
    startAt: item.startAt,
    endAt: item.endAt,
    activityId: item.activityId,
    projectId: item.projectId,
    tagIds: item.tagIds,
    note: item.note,
    billable: item.billable,
    costCenterId: item.costCenterId,
  }
  entryDialogOpen.value = true
}

function openEdit(e: TimeEntry) {
  editing.value = e
  entryDialogOpen.value = true
}

// Calendar: clicking a block edits it in a popover next to the block.
const popoverOpen = ref(false)
const popoverId = ref<string | null>(null)
/** The current version of the entry (it reloads after saves and drags). */
const popoverEntry = computed(() => entries.value.find((e) => e.id === popoverId.value) ?? null)
const popoverAnchor = shallowRef<Anchor | null>(null)
/** Times of the entry being dragged in the calendar, live. */
const dragTimes = ref<{ id: string; startAt: number; endAt: number | null } | null>(null)
/** A click on a block opens its popover; a second click closes it. */
function openPopover(e: TimeEntry, anchor: Anchor) {
  if (popoverOpen.value && popoverId.value === e.id) {
    popoverOpen.value = false
    return
  }
  popoverId.value = e.id
  popoverAnchor.value = anchor
  popoverOpen.value = true
}

/** Calendar drag: new times, everything else unchanged. */
async function resizeEntry(e: TimeEntry, startAt: number, endAt: number | null): Promise<boolean> {
  try {
    await call(
      commands.entryUpdate(e.id, {
        activityId: e.activityId,
        projectId: e.projectId,
        startAt,
        endAt,
        tz: e.tz,
        note: e.note,
        billable: e.billable,
        tagIds: e.tagIds,
        costCenterId: e.costCenterId,
      }),
    )
    return true
  } catch (err) {
    toast.error(errorMessage(err))
    return false
  }
}

async function run<T>(promise: Promise<T>) {
  try {
    await promise
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const duplicate = (e: TimeEntry) => run(call(commands.entryDuplicate(e.id)))
const continueEntry = (e: TimeEntry) => run(timer.continueEntry(e))

const deleting = ref<TimeEntry | null>(null)
const confirmOpen = ref(false)
function askDelete(e: TimeEntry) {
  deleting.value = e
  confirmOpen.value = true
}
async function confirmDelete() {
  if (deleting.value) await run(call(commands.entryDelete(deleting.value.id)))
}

// Table selection and bulk actions
const selected = ref(new Set<string>())
const visibleEntries = computed(() => groups.value.flatMap((g) => g.entries))
/** Selected entries still shown (navigation or deletion drops the rest). */
const selectedEntries = computed(() => visibleEntries.value.filter((e) => selected.value.has(e.id)))
watch([range, view], () => (selected.value = new Set()))

function setSelected(list: TimeEntry[], on: boolean) {
  const next = new Set(selected.value)
  for (const e of list) {
    if (on) next.add(e.id)
    else next.delete(e.id)
  }
  selected.value = next
}
/** Header checkbox: all, some (indeterminate) or none of `list` selected. */
function checkState(list: TimeEntry[]): boolean | 'indeterminate' {
  const count = list.filter((e) => selected.value.has(e.id)).length
  return count === 0 ? false : count === list.length ? true : 'indeterminate'
}

/** Tags on any selected entry, for "remove tag". */
const selectedTags = computed(() => {
  const ids = new Set(selectedEntries.value.flatMap((e) => e.tagIds))
  return tags.all.filter((t) => ids.has(t.id))
})

async function bulkUpdate(patch: EntryPatch) {
  const ids = selectedEntries.value.map((e) => e.id)
  try {
    const result = await call(commands.entryBulkUpdate(ids, patch))
    toast.success(t('timesheet.bulk.updated', result.changed))
    if (result.skippedLocked) toast.warning(t('timesheet.bulk.skippedLocked', result.skippedLocked))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const bulkDeleteOpen = ref(false)
async function bulkDelete() {
  const ids = selectedEntries.value.map((e) => e.id)
  try {
    const result = await call(commands.entryBulkDelete(ids))
    toast.success(t('timesheet.bulk.deleted', result.changed))
    if (result.skippedLocked) toast.warning(t('timesheet.bulk.skippedLocked', result.skippedLocked))
    selected.value = new Set()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const splitting = ref<TimeEntry | null>(null)
const splitOpen = ref(false)
const splitTime = ref('')
function openSplit(e: TimeEntry) {
  splitting.value = e
  splitTime.value = toTimeInput(e.startAt + durationMs(e) / 2)
  splitOpen.value = true
}
async function confirmSplit() {
  const e = splitting.value
  if (!e) return
  let at = fromInputs(toDateInput(e.startAt), splitTime.value)
  if (at <= e.startAt) at = addDays(at, 1) // entry crosses midnight
  if (Number.isNaN(at)) {
    toast.error(t('entry.invalidTime'))
    return
  }
  try {
    await call(commands.entrySplit(e.id, at))
    splitOpen.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <PageHeader :title="title">
    <Tabs v-model="view">
      <TabsList>
        <TabsTrigger value="calendar" :title="$t('timesheet.calendar')"><CalendarDays /><span class="sr-only">{{ $t('timesheet.calendar') }}</span></TabsTrigger>
        <TabsTrigger value="table" :title="$t('timesheet.table')"><List /><span class="sr-only">{{ $t('timesheet.table') }}</span></TabsTrigger>
      </TabsList>
    </Tabs>
    <Tabs v-model="mode">
      <TabsList>
        <TabsTrigger value="day">{{ $t('timesheet.day') }}</TabsTrigger>
        <TabsTrigger value="week">{{ $t('timesheet.week') }}</TabsTrigger>
      </TabsList>
    </Tabs>
    <div class="flex items-center">
      <Button size="icon" variant="ghost" :title="$t('timesheet.previous')" @click="move(-1)"><ChevronLeft /></Button>
      <Button size="sm" variant="outline" @click="anchor = startOfDay(Date.now())">{{ $t('timesheet.today') }}</Button>
      <Button size="icon" variant="ghost" :title="$t('timesheet.next')" @click="move(1)"><ChevronRight /></Button>
    </div>
    <Button @click="openNew()"><Plus /> {{ $t('timesheet.addEntry') }}</Button>
  </PageHeader>

  <!-- Filters: what the calendar, the table and the totals show. -->
  <div class="mb-3 flex flex-wrap items-center gap-2">
    <MultiFilter v-model="filter.kinds" :label="$t('timesheet.show')" :options="kindOptions" />
    <MultiFilter v-model="filter.activityIds" :label="$t('entry.activity')" :options="activityOptions" />
    <MultiFilter v-model="filter.projectIds" :label="$t('entry.project')" :options="projectOptions" />
    <MultiFilter v-if="tags.all.length" v-model="filter.tagIds" :label="$t('entry.tags')" :options="tagOptions" />
    <Select v-model="filter.billable">
      <SelectTrigger size="sm" class="w-40"><SelectValue /></SelectTrigger>
      <SelectContent>
        <SelectItem value="all">{{ $t('reports.billableAll') }}</SelectItem>
        <SelectItem value="yes">{{ $t('reports.billable') }}</SelectItem>
        <SelectItem value="no">{{ $t('reports.notBillable') }}</SelectItem>
      </SelectContent>
    </Select>
    <Button v-if="hasFilter" size="sm" variant="ghost" @click="clearFilter"><X /> {{ $t('reports.clearFilter') }}</Button>
  </div>

  <DayTimeline
    v-if="view === 'calendar' && mode === 'day'"
    class="flex-1"
    :day="days[0]"
    :entries="shownEntries"
    :total="totalOf(0)"
    :now="timer.now"
    :events="shownTimeline"
    :others="shownOthers"
    :candidates="candidates"
    :selected="shownIds"
    @update:selected="peopleIds = $event"
    @edit="openPopover"
    @create="createAt"
    @track="track"
  />

  <EntryCalendar
    v-else-if="view === 'calendar'"
    class="min-h-80 flex-1"
    :days="days"
    :entries="shownEntries"
    :totals="days.map((_, i) => totalOf(i))"
    :now="timer.now"
    :events="shownTimeline"
    @edit="openPopover"
    @create="openNew"
    @resize="resizeEntry"
    @preview="dragTimes = $event"
    @track="track"
  />

  <EmptyState v-else-if="groups.every((g) => g.entries.length === 0 && g.events.length === 0)" :icon="Clock" :text="hasFilter ? $t('timesheet.noMatch') : $t('timesheet.empty')">
    <Button v-if="hasFilter" variant="outline" @click="clearFilter"><X /> {{ $t('reports.clearFilter') }}</Button>
    <Button v-else variant="outline" @click="openNew()"><Plus /> {{ $t('timesheet.addEntry') }}</Button>
  </EmptyState>

  <template v-else>
  <!-- Constant room below the table, so the last rows can scroll above the bar. -->
  <div class="mb-16 overflow-x-auto rounded-lg border">
    <table class="w-full text-sm">
      <thead class="text-left text-xs text-muted-foreground">
        <tr class="border-b">
          <th class="w-8 py-2 pr-0 pl-3">
            <Checkbox
              class="flex"
              :model-value="checkState(visibleEntries)"
              :aria-label="$t('timesheet.bulk.selectAll')"
              @update:model-value="setSelected(visibleEntries, $event === true)"
            />
          </th>
          <th class="w-28 px-3 py-2 font-medium">{{ $t('timesheet.time') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.activity') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.project') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.tags') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.note') }}</th>
          <th class="w-20 px-3 py-2 text-right font-medium">{{ $t('timesheet.duration') }}</th>
          <th class="w-40 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
        </tr>
      </thead>
      <tbody v-for="group in groups" :key="group.day.from">
        <tr class="border-b bg-muted/40">
          <th class="w-8 py-1.5 pr-0 pl-3">
            <Checkbox
              v-if="group.entries.length"
              class="flex"
              :model-value="checkState(group.entries)"
              :aria-label="$t('timesheet.bulk.selectDay')"
              @update:model-value="setSelected(group.entries, $event === true)"
            />
          </th>
          <th colspan="7" class="px-3 py-1.5 text-left font-normal">
            <div class="flex items-center justify-between gap-3">
              <span class="font-medium">{{ format.day(group.day.from) }}</span>
              <DayStatus v-if="group.work" :day="group.work" :holiday-name="holidayName(group.work.date)" />
              <span v-else class="text-muted-foreground">
                {{ $t('timesheet.dayTotal') }}:
                <span class="font-medium tabular-nums text-foreground">{{ settings.formatDuration(group.totalMs) }} h</span>
              </span>
            </div>
          </th>
        </tr>
        <tr
          v-for="item in group.events"
          :key="`${item.addonId}/${item.id}`"
          class="group cursor-pointer border-b border-dashed text-muted-foreground last:border-b-0 hover:bg-muted/30"
          :title="$t('timesheet.trackEvent')"
          @click="track(item)"
        >
          <td class="w-8 py-1.5 pr-0 pl-3" />
          <td class="whitespace-nowrap px-3 py-1.5 tabular-nums">{{ format.time(item.startAt) }}–{{ format.time(item.endAt) }}</td>
          <td class="max-w-44 px-3 py-1.5">
            <ActivityLabel v-if="item.activityId" :activity="activities.byId.get(item.activityId)" />
            <span v-else>–</span>
          </td>
          <td class="max-w-56 px-3 py-1.5">
            <ProjectDot v-if="item.projectId" :project="projects.byId.get(item.projectId) ?? null" show-alias />
            <span v-else>–</span>
          </td>
          <td class="px-3 py-1.5"><TagList :ids="item.tagIds" /></td>
          <td class="max-w-0 w-full truncate px-3 py-1.5" :title="item.detail ?? item.title">
            <span class="font-medium text-foreground">{{ item.title }}</span><span v-if="item.detail"> · {{ item.detail }}</span>
          </td>
          <td class="whitespace-nowrap px-3 py-1.5 text-right tabular-nums">{{ settings.formatDuration(item.endAt - item.startAt) }}</td>
          <td class="px-1 py-0.5" @click.stop>
            <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
              <Button size="icon" variant="ghost" class="size-7" :title="$t('timesheet.trackEvent')" @click="track(item)"><Play /></Button>
            </div>
          </td>
        </tr>
        <tr
          v-for="entry in group.entries"
          :key="entry.id"
          class="group cursor-pointer border-b last:border-b-0 hover:bg-muted/30"
          :class="selected.has(entry.id) ? 'bg-muted/50' : ''"
          @click="openEdit(entry)"
        >
          <td class="w-8 py-1.5 pr-0 pl-3" @click.stop>
            <Checkbox
              class="flex"
              :model-value="selected.has(entry.id)"
              :aria-label="$t('timesheet.bulk.select')"
              @update:model-value="setSelected([entry], $event === true)"
            />
          </td>
          <td class="whitespace-nowrap px-3 py-1.5 tabular-nums text-muted-foreground">
            {{ format.time(entry.startAt) }}–{{ entry.endAt === null ? '' : format.time(entry.endAt) }}
            <sup
              v-if="entry.endAt !== null && daysBetween(entry.startAt, entry.endAt) > 0"
              class="text-[10px] font-medium text-foreground"
              :title="format.day(entry.endAt)"
            >+{{ daysBetween(entry.startAt, entry.endAt) }}</sup>
          </td>
          <td class="max-w-44 px-3 py-1.5">
            <ActivityLabel :activity="entry.activityId ? activities.byId.get(entry.activityId) : null" />
          </td>
          <td class="max-w-56 px-3 py-1.5">
            <ProjectDot :project="entry.projectId ? projects.byId.get(entry.projectId) : null" show-alias />
          </td>
          <td class="px-3 py-1.5"><TagList :ids="entry.tagIds" /></td>
          <td class="max-w-0 w-full truncate px-3 py-1.5 text-muted-foreground" :title="entry.note">{{ entry.note }}</td>
          <td
            class="whitespace-nowrap px-3 py-1.5 text-right font-medium tabular-nums"
            :class="entry.endAt === null ? 'text-green-600 dark:text-green-400' : ''"
          >
            <span v-if="entry.billable" class="mr-1 text-xs font-normal text-muted-foreground" :title="$t('entry.billable')">€</span>
            {{ settings.formatDuration(durationMs(entry)) }}
          </td>
          <td class="px-1 py-0.5" @click.stop>
            <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
              <Button v-if="entry.endAt !== null" size="icon" variant="ghost" class="size-7" :title="$t('timer.continue')" @click="continueEntry(entry)"><Play /></Button>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="openEdit(entry)"><Pencil /></Button>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('timesheet.split')" :disabled="entry.locked" @click="openSplit(entry)"><Scissors /></Button>
              <Button v-if="entry.endAt !== null" size="icon" variant="ghost" class="size-7" :title="$t('timesheet.duplicate')" @click="duplicate(entry)"><Copy /></Button>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" :disabled="entry.locked" @click="askDelete(entry)"><Trash2 /></Button>
            </div>
          </td>
        </tr>
        <tr v-if="group.entries.length === 0 && group.events.length === 0" class="border-b last:border-b-0">
          <td colspan="8" class="px-3 py-6 text-center text-muted-foreground">{{ $t('timesheet.empty') }}</td>
        </tr>
      </tbody>
    </table>
  </div>
  <!-- Bulk actions: float at the bottom of the page. The zero-height wrapper
       keeps the table from moving; `mt-auto` (the page is a full-height
       column) puts it at the bottom on short tables, `sticky` on long ones. -->
  <div class="pointer-events-none sticky bottom-6 z-20 mt-auto h-0">
    <Transition
      enter-from-class="translate-y-2 opacity-0"
      leave-to-class="translate-y-2 opacity-0"
      enter-active-class="transition duration-150"
      leave-active-class="transition duration-150"
    >
    <div
      v-if="selectedEntries.length"
      class="pointer-events-auto absolute bottom-0 left-1/2 flex w-max max-w-full -translate-x-1/2 flex-wrap items-center gap-2 rounded-lg border bg-background px-3 py-2 text-sm shadow-lg"
    >
      <span class="font-medium">{{ $t('timesheet.bulk.selected', selectedEntries.length) }}</span>
      <div class="flex flex-wrap items-center gap-2">
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button size="sm" variant="outline">{{ $t('entry.activity') }} <ChevronDown /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" class="max-h-72 overflow-y-auto">
            <DropdownMenuItem v-for="a in activities.active" :key="a.id" @select="bulkUpdate({ activityId: a.id })">
              <ActivityLabel :activity="a" />
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button size="sm" variant="outline">{{ $t('entry.project') }} <ChevronDown /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" class="max-h-72 overflow-y-auto">
            <DropdownMenuItem @select="bulkUpdate({ setProject: true, projectId: null })">
              <ProjectDot :project="null" />
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem v-for="p in projects.active" :key="p.id" @select="bulkUpdate({ setProject: true, projectId: p.id })">
              <ProjectDot :project="p" show-alias />
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button size="sm" variant="outline">{{ $t('entry.tags') }} <ChevronDown /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>{{ $t('timesheet.bulk.addTag') }}</DropdownMenuSubTrigger>
              <DropdownMenuSubContent class="max-h-72 overflow-y-auto">
                <DropdownMenuItem v-for="tag in tags.all" :key="tag.id" @select="bulkUpdate({ addTagIds: [tag.id] })">
                  <span class="size-2.5 rounded-full" :style="{ backgroundColor: tag.color }" />
                  {{ tag.name }}
                  <span v-if="tag.projectId" class="text-xs text-muted-foreground">· {{ projects.byId.get(tag.projectId)?.name }}</span>
                </DropdownMenuItem>
                <DropdownMenuItem v-if="tags.all.length === 0" disabled>{{ $t('common.none') }}</DropdownMenuItem>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger :disabled="selectedTags.length === 0">{{ $t('timesheet.bulk.removeTag') }}</DropdownMenuSubTrigger>
              <DropdownMenuSubContent class="max-h-72 overflow-y-auto">
                <DropdownMenuItem v-for="tag in selectedTags" :key="tag.id" @select="bulkUpdate({ removeTagIds: [tag.id] })">
                  <span class="size-2.5 rounded-full" :style="{ backgroundColor: tag.color }" />
                  {{ tag.name }}
                </DropdownMenuItem>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button size="sm" variant="outline">{{ $t('entry.billable') }} <ChevronDown /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem @select="bulkUpdate({ billable: true })">{{ $t('reports.billable') }}</DropdownMenuItem>
            <DropdownMenuItem @select="bulkUpdate({ billable: false })">{{ $t('reports.notBillable') }}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button size="sm" variant="outline" class="text-destructive" @click="bulkDeleteOpen = true"><Trash2 /> {{ $t('common.delete') }}</Button>
        <Button size="icon" variant="ghost" class="size-7" :title="$t('timesheet.bulk.clear')" :aria-label="$t('timesheet.bulk.clear')" @click="selected = new Set()">
          <X />
        </Button>
      </div>
    </div>
    </Transition>
  </div>
  </template>

  <EntryPopover
    v-model:open="popoverOpen"
    :entry="popoverEntry"
    :reference="popoverAnchor"
    :times="dragTimes?.id === popoverId ? dragTimes : null"
    @continue="continueEntry"
    @duplicate="duplicate"
    @split="openSplit"
    @delete="askDelete"
    @more="openEdit"
  />
  <EntryDialog v-model:open="entryDialogOpen" :entry="editing" :day="newEntryDay" :draft="draft" />
  <ConfirmDialog v-model:open="confirmOpen" :title="$t('timesheet.deleteConfirm')" @confirm="confirmDelete" />
  <ConfirmDialog
    v-model:open="bulkDeleteOpen"
    :title="$t('timesheet.bulk.deleteConfirm', selectedEntries.length)"
    @confirm="bulkDelete"
  />
  <Dialog v-model:open="splitOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-xs">
      <DialogHeader>
        <DialogTitle>{{ $t('timesheet.split') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="confirmSplit">
        <div class="grid gap-2">
          <Label for="split-at">{{ $t('timesheet.splitAt') }}</Label>
          <TimeInput id="split-at" v-model="splitTime" required />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="splitOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('timesheet.split') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
