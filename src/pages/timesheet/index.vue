<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { useStorage } from '@vueuse/core'
import { CalendarDays, ChevronDown, ChevronLeft, ChevronRight, Clock, Copy, List, Pencil, Play, Plus, Scissors, Trash2, X } from '@lucide/vue'
import { commands, type EntryPatch, type TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
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
import EntryCalendar from '@/components/time/EntryCalendar.vue'
import EntryDialog from '@/components/time/EntryDialog.vue'
import EntryPopover from '@/components/time/EntryPopover.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import TagList from '@/components/time/TagList.vue'
import { useEntries } from '@/composables/useEntries'
import { useWorkDays } from '@/composables/useWorkDays'
import DayStatus from '@/components/time/DayStatus.vue'
import { useFormat } from '@/composables/useFormat'
import type { Anchor } from '@/lib/anchor'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, dayRange, daysBetween, fromInputs, startOfDay, toDateInput, toTimeInput, weekDays } from '@/lib/time'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTagsStore } from '@/stores/tags'
import { useTimerStore } from '@/stores/timer'

definePage({ meta: { titleKey: 'nav.timesheet', fill: true } })

const { t } = useI18n()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const settings = useSettingsStore()
const tags = useTagsStore()
const timer = useTimerStore()
const format = useFormat()

// Remembered: day/week and calendar/table across restarts; the shown date
// only while the app runs (a restart starts on today again).
const mode = useStorage<'day' | 'week'>('timesheet.mode', 'day')
const view = useStorage<'table' | 'calendar'>('timesheet.viewMode', 'calendar')
const anchor = useStorage('timesheet.anchor', startOfDay(Date.now()), sessionStorage)

const days = computed(() => (mode.value === 'day' ? [dayRange(anchor.value)] : weekDays(anchor.value)))
const range = computed(() => ({ from: days.value[0].from, to: days.value[days.value.length - 1].to }))
const { entries, summaries } = useEntries(range, days)
const { byDate, holidayName } = useWorkDays(range)

const title = computed(() =>
  mode.value === 'day'
    ? format.day(anchor.value)
    : `${format.shortDate(range.value.from)} – ${format.longDate(days.value[6].from)}`,
)

function move(direction: -1 | 1) {
  anchor.value = addDays(anchor.value, direction * (mode.value === 'day' ? 1 : 7))
}

/** Entries grouped by the day they start, newest day first. */
const groups = computed(() =>
  days.value
    .map((day, i) => ({
      day,
      work: byDate(toDateInput(day.from)),
      totalMs: summaries.value[i]?.totalMs ?? 0,
      entries: entries.value.filter((e) => e.startAt >= day.from && e.startAt < day.to).reverse(),
    }))
    .filter((g) => mode.value === 'day' || g.entries.length > 0)
    .reverse(),
)

function durationMs(e: TimeEntry) {
  return (e.endAt ?? timer.now) - e.startAt
}

// Dialogs
const entryDialogOpen = ref(false)
const editing = ref<TimeEntry | null>(null)
const newEntryDay = ref<number>(Date.now())

function openNew(day?: number) {
  editing.value = null
  newEntryDay.value = day ?? (mode.value === 'day' ? anchor.value : Date.now())
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

  <EntryCalendar
    v-if="view === 'calendar'"
    class="min-h-80 flex-1"
    :days="days"
    :entries="entries"
    :totals="days.map((_, i) => summaries[i]?.totalMs ?? 0)"
    :now="timer.now"
    @edit="openPopover"
    @create="openNew"
    @resize="resizeEntry"
    @preview="dragTimes = $event"
  />

  <EmptyState v-else-if="groups.every((g) => g.entries.length === 0)" :icon="Clock" :text="$t('timesheet.empty')">
    <Button variant="outline" @click="openNew()"><Plus /> {{ $t('timesheet.addEntry') }}</Button>
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
        <tr v-if="group.entries.length === 0" class="border-b last:border-b-0">
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
  <EntryDialog v-model:open="entryDialogOpen" :entry="editing" :day="newEntryDay" />
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
