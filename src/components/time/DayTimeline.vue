<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { AddonTimelineItem, TimeEntry } from '@/bindings'
import { useFormat } from '@/composables/useFormat'
import { type Anchor, pointAnchor } from '@/lib/anchor'
import type { PersonDay } from '@/lib/busy'
import { textOn } from '@/lib/colors'
import { layoutLanes } from '@/lib/lanes'
import { HOUR, fromInputs, toDateInput, type DayRange } from '@/lib/time'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { useOrgStore } from '@/stores/org'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'

// One day as a horizontal timeline: hours run left to right across the whole
// width, entries are bars sized by duration. Entries that do not overlap use
// the full height; overlapping ones share it. The hours shown are 07:00 to
// 20:00, widened to fit entries and the current time. The people a manager
// added (timesheet filter bar) each get a row of their own under the
// person's own (read only).

const props = defineProps<{
  day: DayRange
  entries: TimeEntry[]
  /** Tracked time of the day in ms. */
  total: number
  now: number
  /** Addon timeline items, e.g. calendar events. */
  events?: AddonTimelineItem[]
  /** Other people's rows, in the order shown. */
  others?: PersonDay[]
}>()

const emit = defineEmits<{
  /** Click on a bar; `anchor` is where it was clicked, for a popover. */
  edit: [entry: TimeEntry, anchor: Anchor]
  /** Double-click on an empty spot: the full hour under the pointer. */
  create: [at: number]
  /** Click on a timed timeline item: create an entry from it. */
  track: [item: AddonTimelineItem, anchor: Anchor]
}>()

const { t } = useI18n()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const settings = useSettingsStore()
const format = useFormat()

const DEFAULT_FIRST = 7
const DEFAULT_LAST = 20

const segmentsOf = (entries: TimeEntry[]) =>
  entries.map((entry) => ({
    entry,
    start: Math.max(entry.startAt, props.day.from),
    end: Math.min(entry.endAt ?? props.now, props.day.to),
  }))
const segments = computed(() => segmentsOf(props.entries))
const otherSegments = computed(() =>
  (props.others ?? []).map((o) => ({
    ...o,
    segments: segmentsOf(o.entries),
    // When they are busy that day (shown behind what they tracked).
    busySegments: o.busy
      .filter((b) => b.from < props.day.to && b.to > props.day.from)
      .map((b) => ({ ...b, start: Math.max(b.from, props.day.from), end: Math.min(b.to, props.day.to) })),
  })),
)
const withOthers = computed(() => otherSegments.value.length > 0)
const eventSegments = computed(() =>
  (props.events ?? [])
    .filter((i) => !i.allDay && i.startAt < props.day.to && i.endAt > props.day.from)
    .map((item) => ({ item, start: Math.max(item.startAt, props.day.from), end: Math.min(item.endAt, props.day.to) })),
)
const allDay = computed(() => (props.events ?? []).filter((i) => i.allDay && i.startAt < props.day.to && i.endAt > props.day.from))

const isToday = computed(() => props.now >= props.day.from && props.now < props.day.to)

/** First and last (exclusive) hour shown. */
const shown = computed(() => {
  const spans = [...segments.value, ...otherSegments.value.flatMap((o) => [...o.segments, ...o.busySegments.filter((b) => b.kind === 'meeting')]), ...eventSegments.value].map((s) => [s.start, s.end] as const)
  if (isToday.value) spans.push([props.now, props.now])
  let first = DEFAULT_FIRST
  let last = DEFAULT_LAST
  for (const [from, to] of spans) {
    first = Math.min(first, Math.floor((from - props.day.from) / HOUR))
    last = Math.max(last, Math.ceil((to - props.day.from) / HOUR))
  }
  return { first: Math.max(0, first), last: Math.min(24, last) }
})
const hourCount = computed(() => shown.value.last - shown.value.first)
const hourLabels = computed(() =>
  Array.from({ length: hourCount.value }, (_, i) => {
    const h = shown.value.first + i
    return {
      h,
      left: (i / hourCount.value) * 100,
      label: format.time(fromInputs(toDateInput(props.day.from), `${String(h).padStart(2, '0')}:00`)),
    }
  }),
)

function x(ms: number) {
  return (((ms - props.day.from) / HOUR - shown.value.first) / hourCount.value) * 100
}

function place<T extends { start: number; end: number; lane: number; lanes: number }>(b: T) {
  const left = x(b.start)
  return {
    left: `${left}%`,
    width: `calc(${Math.max(x(b.end) - left, 0.4)}% - 2px)`,
    top: `calc(${(b.lane / b.lanes) * 100}% + 2px)`,
    height: `calc(${100 / b.lanes}% - 4px)`,
  }
}

function blocksOf(segs: ReturnType<typeof segmentsOf>) {
  return layoutLanes(segs).map((b) => {
    const activity = b.entry.activityId ? activities.byId.get(b.entry.activityId) : undefined
    const project = b.entry.projectId ? projects.byId.get(b.entry.projectId) : undefined
    const color = activity?.color ?? project?.color
    return {
      ...b,
      activity,
      project,
      style: {
        ...place(b),
        backgroundColor: color ?? 'var(--muted)',
        color: color ? textOn(color) : 'var(--foreground)',
      },
      // Fewer rows where the bar is crowded.
      roomy: b.lanes <= 2,
    }
  })
}

/** The person's own row first, then the others'. */
const LANE_HEIGHT = 56
const org = useOrgStore()
const rows = computed(() => {
  const own = { id: 'me', name: t('timesheet.me'), mine: true, total: props.total, blocks: blocksOf(segments.value) }
  const others = otherSegments.value.map((o) => ({
    id: o.id,
    name: o.name,
    mine: false,
    total: o.segments.reduce((n, sg) => n + Math.max(0, sg.end - sg.start), 0),
    blocks: blocksOf(o.segments),
    busy: o.busySegments.map((b) => {
      const left = x(b.start)
      return {
        key: `${b.kind}/${b.from}`,
        kind: b.kind,
        label: b.kind === 'absence' ? t('timesheet.absent') : t('timesheet.busy'),
        title: `${format.time(b.start)}–${format.time(b.end)} · ${b.kind === 'absence' ? t('timesheet.absent') : t('timesheet.busy')}`,
        style: { left: `${left}%`, width: `calc(${Math.max(x(b.end) - left, 0.4)}% - 2px)`, top: '2px', bottom: '2px' },
      }
    }),
  }))
  return [{ ...own, busy: [] as (typeof others)[number]['busy'] }, ...others].map((r) => ({
    ...r,
    // Alone, the row fills the page; with others each row is as tall as its lanes need.
    minHeight: withOthers.value ? `${Math.max(2, ...r.blocks.map((b) => b.lanes)) * LANE_HEIGHT + 8}px` : undefined,
  }))
})

const eventBlocks = computed(() =>
  layoutLanes(eventSegments.value).map((b) => {
    const color = b.item.color ?? 'var(--muted-foreground)'
    return { ...b, style: { ...place(b), borderColor: color, backgroundColor: `color-mix(in srgb, ${color} 12%, transparent)` } }
  }),
)

const nowLeft = computed(() => Math.min(100, Math.max(0, x(props.now))))

function timeRange(e: TimeEntry) {
  return `${format.time(e.startAt)}–${e.endAt === null ? '' : format.time(e.endAt)}`
}

function tooltip(e: TimeEntry, activityName?: string, projectName?: string) {
  return [timeRange(e), activityName, projectName, e.billable ? `€ ${t('entry.billable')}` : null, e.note].filter(Boolean).join('\n')
}

function eventTooltip(item: AddonTimelineItem) {
  return [`${format.time(item.startAt)}–${format.time(item.endAt)}`, item.title, item.detail, t('timesheet.trackEvent')]
    .filter(Boolean)
    .join('\n')
}

function onBlockClick(entry: TimeEntry, event: MouseEvent) {
  const el = event.currentTarget as HTMLElement
  // Keyboard activation has no pointer position: use the bar's top.
  const pointer = event.detail !== 0
  emit('edit', entry, pointAnchor(el, pointer ? event.clientY : el.getBoundingClientRect().top, pointer ? event.clientX : undefined))
}

/** A click on a timeline item (a meeting, an event): opens it where it was clicked, like an entry. */
function clickItem(item: AddonTimelineItem, event: MouseEvent) {
  const el = event.currentTarget as HTMLElement
  const pointer = event.detail !== 0
  emit('track', item, pointAnchor(el, pointer ? event.clientY : el.getBoundingClientRect().top, pointer ? event.clientX : undefined))
}
function onTrackDblClick(event: MouseEvent) {
  const el = event.currentTarget as HTMLElement
  if (event.target !== el) return
  const rect = el.getBoundingClientRect()
  const hour = Math.min(
    shown.value.last - 1,
    shown.value.first + Math.floor(((event.clientX - rect.left) / rect.width) * hourCount.value),
  )
  emit('create', fromInputs(toDateInput(props.day.from), `${String(hour).padStart(2, '0')}:00`))
}
</script>

<template>
  <div data-day-timeline class="flex min-h-80 flex-col overflow-auto rounded-lg border">
    <div class="flex min-w-[720px] flex-1 flex-col">
      <div class="flex items-baseline justify-between gap-3 border-b px-3 py-1.5 text-xs">
        <span class="font-medium">{{ format.day(day.from) }}</span>
        <span class="tabular-nums text-muted-foreground">{{ settings.formatDuration(total) }}</span>
      </div>
      <ul v-if="allDay.length" class="grid gap-1 border-b px-3 py-1.5">
        <li
          v-for="item in allDay"
          :key="`${item.addonId}/${item.id}`"
          class="truncate rounded-sm px-2 py-0.5 text-xs leading-4"
          :class="item.addonId === 'zeima.absences' ? 'absent-hatch' : 'border border-dashed'"
          :style="item.addonId === 'zeima.absences' ? undefined : { borderColor: item.color ?? 'var(--border)' }"
          :title="eventTooltip(item)"
        >{{ item.title }}</li>
      </ul>

      <!-- Hours -->
      <div class="relative mr-3 h-5 border-b" :class="withOthers ? 'ml-[calc(0.75rem+128px)]' : 'ml-3'">
        <span
          v-for="(label, i) in hourLabels"
          :key="label.h"
          class="absolute top-1 text-[10px] leading-3 tabular-nums text-muted-foreground"
          :class="i === 0 ? '' : '-translate-x-1/2'"
          :style="{ left: `${label.left}%` }"
        >{{ label.label }}</span>
      </div>

      <!-- Tracks: the person's own, then one per added person -->
      <div class="mx-3 mb-3 flex min-h-40 flex-1 flex-col gap-2">
        <div v-for="row in rows" :key="row.id" class="flex min-h-0 flex-1 gap-2" :style="{ minHeight: row.minHeight }">
          <div v-if="withOthers" class="w-30 shrink-0 pt-1.5 text-sm leading-4">
            <p class="flex items-center gap-1.5 truncate font-medium">
              <UserAvatar :user-id="row.mine ? org.myId : row.id" :name="row.name" size="xs" />
              <span class="truncate">{{ row.name }}</span>
            </p>
            <p class="text-xs tabular-nums text-muted-foreground">{{ settings.formatDuration(row.total) }}</p>
          </div>
          <div class="relative min-w-0 flex-1" @dblclick="row.mine && onTrackDblClick($event)">
            <div
              v-for="label in hourLabels"
              :key="label.h"
              class="pointer-events-none absolute inset-y-0 border-l border-dashed border-border/70"
              :style="{ left: `${label.left}%` }"
            />
            <template v-if="row.mine">
              <button
                v-for="ev in eventBlocks"
                :key="`${ev.item.addonId}/${ev.item.id}`"
                type="button"
                class="absolute flex flex-col justify-start overflow-hidden rounded-md border border-dashed px-1.5 py-1 text-left text-[11px] leading-tight text-foreground transition hover:brightness-95 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none dark:hover:brightness-125"
                :style="ev.style"
                :title="eventTooltip(ev.item)"
                :aria-label="`${$t('timesheet.trackEvent')}: ${ev.item.title}`"
                @click="clickItem(ev.item, $event)"
              >
                <span class="truncate font-medium">{{ ev.item.title }}</span>
                <span v-if="ev.item.detail" class="truncate text-muted-foreground">{{ ev.item.detail }}</span>
              </button>
            </template>
            <div
              v-for="b in row.busy"
              :key="b.key"
              class="absolute overflow-hidden rounded-md px-1.5 py-1 text-[11px] italic leading-tight"
              :class="b.kind === 'absence' ? 'absent-hatch' : 'border border-dashed border-muted-foreground/50 bg-muted/60 text-muted-foreground'"
              :style="b.style"
              :title="b.title"
            >{{ b.label }}</div>
            <component
              :is="row.mine ? 'button' : 'div'"
              v-for="b in row.blocks"
              :key="b.entry.id"
              :type="row.mine ? 'button' : undefined"
              class="absolute flex flex-col justify-start gap-0.5 overflow-hidden rounded-md px-2 py-1 text-left text-[11px] leading-tight shadow-xs"
              :class="[
                b.entry.endAt === null ? 'ring-1 ring-green-500/60' : '',
                row.mine ? 'transition hover:brightness-95 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none dark:hover:brightness-125' : '',
              ]"
              :style="b.style"
              :title="tooltip(b.entry, b.activity?.name, b.project?.name)"
              @click="row.mine && onBlockClick(b.entry, $event)"
            >
              <span class="flex min-w-0 items-start gap-1">
                <span class="truncate font-medium">{{ b.activity?.name ?? $t('activities.none') }}</span>
                <span
                  v-if="b.entry.billable"
                  class="ml-auto shrink-0 rounded-sm bg-background/90 px-1 text-[10px] font-semibold text-foreground"
                  :title="$t('entry.billable')"
                >€</span>
              </span>
              <span
                v-if="b.project"
                class="flex w-fit max-w-full min-w-0 items-center gap-1 rounded-sm bg-background/90 px-1.5 py-0.5 font-medium text-foreground"
              >
                <span class="truncate">{{ b.project.name }}</span>
                <span v-if="b.project.alias" class="shrink-0 font-mono text-[10px] text-muted-foreground">{{ b.project.alias }}</span>
              </span>
              <span v-if="b.roomy" class="truncate tabular-nums opacity-75">{{ timeRange(b.entry) }}</span>
              <span v-if="b.roomy && b.entry.note" class="truncate opacity-75">{{ b.entry.note }}</span>
            </component>
            <div
              v-if="isToday"
              class="pointer-events-none absolute inset-y-0 z-[1] border-l-2 border-red-500"
              :style="{ left: `${nowLeft}%` }"
            />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
