<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { useStorage } from '@vueuse/core'
import { CalendarDays, ChevronLeft, ChevronRight, Clock, Copy, List, Pencil, Play, Plus, Scissors, Trash2 } from '@lucide/vue'
import { commands, type TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import TimeInput from '@/components/time/TimeInput.vue'
import { Label } from '@/components/ui/label'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import EntryCalendar from '@/components/time/EntryCalendar.vue'
import EntryDialog from '@/components/time/EntryDialog.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import TagList from '@/components/time/TagList.vue'
import { useEntries } from '@/composables/useEntries'
import { useWorkDays } from '@/composables/useWorkDays'
import DayStatus from '@/components/time/DayStatus.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, dayRange, fromInputs, startOfDay, toDateInput, toTimeInput, weekDays } from '@/lib/time'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTimerStore } from '@/stores/timer'

definePage({ meta: { titleKey: 'nav.timesheet' } })

const { t } = useI18n()
const projects = useProjectsStore()
const settings = useSettingsStore()
const timer = useTimerStore()
const format = useFormat()

const mode = ref<'day' | 'week'>('day')
const view = useStorage<'table' | 'calendar'>('timesheet.viewMode', 'calendar')
const anchor = ref(startOfDay(Date.now()))

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
    :days="days"
    :entries="entries"
    :totals="days.map((_, i) => summaries[i]?.totalMs ?? 0)"
    :now="timer.now"
    @edit="openEdit"
    @create="openNew"
  />

  <EmptyState v-else-if="groups.every((g) => g.entries.length === 0)" :icon="Clock" :text="$t('timesheet.empty')">
    <Button variant="outline" @click="openNew()"><Plus /> {{ $t('timesheet.addEntry') }}</Button>
  </EmptyState>

  <div v-else class="overflow-x-auto rounded-lg border">
    <table class="w-full text-sm">
      <thead class="text-left text-xs text-muted-foreground">
        <tr class="border-b">
          <th class="w-28 px-3 py-2 font-medium">{{ $t('timesheet.time') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.project') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.tags') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.note') }}</th>
          <th class="w-20 px-3 py-2 text-right font-medium">{{ $t('timesheet.duration') }}</th>
          <th class="w-40 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
        </tr>
      </thead>
      <tbody v-for="group in groups" :key="group.day.from">
        <tr class="border-b bg-muted/40">
          <th colspan="6" class="px-3 py-1.5 text-left font-normal">
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
          class="group border-b last:border-b-0 hover:bg-muted/30"
          @dblclick="openEdit(entry)"
        >
          <td class="whitespace-nowrap px-3 py-1.5 tabular-nums text-muted-foreground">
            {{ format.time(entry.startAt) }}–{{ entry.endAt === null ? '' : format.time(entry.endAt) }}
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
          <td class="px-1 py-0.5">
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
          <td colspan="6" class="px-3 py-6 text-center text-muted-foreground">{{ $t('timesheet.empty') }}</td>
        </tr>
      </tbody>
    </table>
  </div>

  <EntryDialog v-model:open="entryDialogOpen" :entry="editing" :day="newEntryDay" />
  <ConfirmDialog v-model:open="confirmOpen" :title="$t('timesheet.deleteConfirm')" @confirm="confirmDelete" />
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
