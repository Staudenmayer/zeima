<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useElementSize } from '@vueuse/core'
import { useI18n } from 'vue-i18n'
import type { TimeEntry } from '@/bindings'
import { useFormat } from '@/composables/useFormat'
import { type Anchor, pointAnchor } from '@/lib/anchor'
import { textOn } from '@/lib/colors'
import { HOUR, fromInputs, toDateInput, type DayRange } from '@/lib/time'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTagsStore } from '@/stores/tags'

// Day/week calendar grid: one column per day, entries as blocks positioned
// by time. Overlapping entries share the column side by side. It fills its
// container and shows the whole day zoomed to VISIBLE_HOURS; scroll for more.
// Blocks take the activity's color (the project's for entries without one).

const props = defineProps<{
  days: DayRange[]
  entries: TimeEntry[]
  /** Day totals in ms, same order as `days`. */
  totals: number[]
  now: number
  /** An edge was dragged: save the new start and end (end `null` while
   * running). Resolves to whether it was saved. Bound as `@resize`. */
  onResize?: (entry: TimeEntry, startAt: number, endAt: number | null) => Promise<boolean>
}>()

const emit = defineEmits<{
  /** Click on a block; `anchor` is where it was clicked, for a popover. */
  edit: [entry: TimeEntry, anchor: Anchor]
  /** Double-click on an empty slot: start of that day. */
  create: [day: number]
  /** Times of the entry being dragged, live; `null` when the drag ends. */
  preview: [times: { id: string; startAt: number; endAt: number | null } | null]
}>()

const { t } = useI18n()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const tags = useTagsStore()
const settings = useSettingsStore()
const format = useFormat()

/** Blocks wider than this get the edit popover at the pointer. */
const POPOVER_ROOM_PX = 320

/** Hours that fit in the visible area. */
const VISIBLE_HOURS = 12
/** Lower bound for very small windows. */
const MIN_HOUR_PX = 32
const MIN_BLOCK_PX = 18

const scroller = ref<HTMLElement | null>(null)
const header = ref<HTMLElement | null>(null)
const { height: viewHeight } = useElementSize(scroller)
const { height: headerHeight } = useElementSize(header, undefined, { box: 'border-box' })
const hourPx = computed(() => Math.max(MIN_HOUR_PX, (viewHeight.value - headerHeight.value) / VISIBLE_HOURS))

// Resizing: drag the top or bottom edge of a block. While dragging, the
// block is laid out from `preview`; it stays until the saved entry arrives.
const SNAP_MS = 5 * 60_000
const preview = ref<{ id: string; startAt: number; endAt: number | null } | null>(null)
let drag: {
  entry: TimeEntry
  edge: 'start' | 'end'
  /** Pointer time minus edge time at the press, so the edge does not jump. */
  grabOffset: number
  moved: boolean
} | null = null

/** Day columns (same order as `columns`), to map the pointer to a time. */
const columnEls: HTMLElement[] = []

/** Time under the pointer, in the day column it is over (or the nearest). */
function pointerTime(event: PointerEvent): number | null {
  if (columnEls.length === 0) return null
  let i = columnEls.findIndex((el) => event.clientX < el.getBoundingClientRect().right)
  if (i === -1) i = columnEls.length - 1
  const rect = columnEls[i].getBoundingClientRect()
  const day = props.days[i]
  if (!day) return null
  const y = Math.min(Math.max(event.clientY - rect.top, 0), rect.height)
  return day.from + (y / hourPx.value) * HOUR
}

/** Scrolls while dragging near the top or bottom edge of the view. */
function autoScroll(event: PointerEvent) {
  const el = scroller.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const zone = 32
  if (event.clientY < rect.top + headerHeight.value + zone) el.scrollTop -= 16
  else if (event.clientY > rect.bottom - zone) el.scrollTop += 16
}
/** A drag ends with a click on the block; it must not open the editor. */
let suppressClick = false

/** Edges that can be dragged: not on locked entries, not the end of a
 * running one, and not where the entry continues past midnight. */
function canResize(day: DayRange, b: Block, edge: 'start' | 'end'): boolean {
  const e = b.entry
  if (e.locked) return false
  if (edge === 'start') return e.startAt >= day.from
  return e.endAt !== null && e.endAt <= day.to
}

function startDrag(event: PointerEvent, b: Block, edge: 'start' | 'end') {
  if (event.button !== 0) return
  const at = pointerTime(event)
  const edgeTime = edge === 'start' ? b.entry.startAt : b.entry.endAt!
  drag = { entry: b.entry, edge, grabOffset: at === null ? 0 : at - edgeTime, moved: false }
  preview.value = { id: b.entry.id, startAt: b.entry.startAt, endAt: b.entry.endAt }
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', onDragEnd)
  window.addEventListener('keydown', onDragKey)
}

function onDragMove(event: PointerEvent) {
  if (!drag || !preview.value) return
  autoScroll(event)
  const at = pointerTime(event)
  if (at === null) return
  const e = drag.entry
  // Across day columns too: entries may span several days.
  const time = Math.round((at - drag.grabOffset) / SNAP_MS) * SNAP_MS
  if (drag.edge === 'start') {
    preview.value = { ...preview.value, startAt: Math.min(time, (e.endAt ?? props.now) - SNAP_MS) }
  } else {
    preview.value = { ...preview.value, endAt: Math.max(time, e.startAt + SNAP_MS) }
  }
  drag.moved = true
}

function stopListening() {
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragEnd)
  window.removeEventListener('keydown', onDragKey)
}

async function onDragEnd() {
  stopListening()
  const d = drag
  const p = preview.value
  drag = null
  if (!d || !p) return
  suppressClick = d.moved
  if (p.startAt === d.entry.startAt && p.endAt === d.entry.endAt) {
    preview.value = null
    return
  }
  const saved = (await props.onResize?.(d.entry, p.startAt, p.endAt)) ?? false
  // Saved: keep the preview until the reloaded entries arrive (no jump back).
  if (!saved && preview.value === p) preview.value = null
}

/** Esc cancels the drag. */
function onDragKey(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  stopListening()
  drag = null
  preview.value = null
}

watch(preview, (p) => emit('preview', p))

// The saved entry (or a failed save's reload) replaces the preview.
watch(
  () => props.entries,
  () => {
    if (!drag) preview.value = null
  },
)

function onBlockClick(entry: TimeEntry, event: MouseEvent) {
  if (suppressClick) {
    suppressClick = false
    return
  }
  const el = event.currentTarget as HTMLElement
  const rect = el.getBoundingClientRect()
  // Keyboard activation has no pointer position: use the block's top.
  const pointer = event.detail !== 0
  const y = pointer ? event.clientY : rect.top
  // Wide blocks (day view) leave no room beside them: open at the pointer,
  // like a context menu. Narrow ones get it beside the block.
  const x = pointer && rect.width > POPOVER_ROOM_PX ? event.clientX : undefined
  emit('edit', entry, pointAnchor(el, y, x))
}

/** Shown in the block: the preview while dragging. */
function shownEntry(e: TimeEntry): TimeEntry {
  return preview.value?.id === e.id ? { ...e, startAt: preview.value.startAt, endAt: preview.value.endAt } : e
}

interface Block {
  entry: TimeEntry
  start: number
  end: number
  lane: number
  lanes: number
}

const columns = computed(() =>
  props.days.map((day) => {
    const segments = props.entries
      .map((e) => (preview.value?.id === e.id ? { ...e, startAt: preview.value.startAt, endAt: preview.value.endAt } : e))
      .map((entry) => ({
        entry,
        start: Math.max(entry.startAt, day.from),
        end: Math.min(entry.endAt ?? props.now, day.to),
      }))
      .filter((s) => s.end > s.start)
      .sort((a, b) => a.start - b.start || b.end - a.end)

    // Greedy lanes within clusters of mutually overlapping entries.
    const blocks: Block[] = []
    let cluster: Block[] = []
    let laneEnds: number[] = []
    let clusterEnd = -Infinity
    const closeCluster = () => {
      for (const b of cluster) b.lanes = laneEnds.length
      cluster = []
      laneEnds = []
    }
    for (const s of segments) {
      if (s.start >= clusterEnd) closeCluster()
      let lane = laneEnds.findIndex((end) => end <= s.start)
      if (lane === -1) lane = laneEnds.push(s.end) - 1
      else laneEnds[lane] = s.end
      const block = { ...s, lane, lanes: 1 }
      cluster.push(block)
      blocks.push(block)
      clusterEnd = Math.max(clusterEnd, s.end)
    }
    closeCluster()
    return { day, blocks }
  }),
)

/** The whole day; the view scrolls. */
const hours = { first: 0, last: 24 }

const height = computed(() => (hours.last - hours.first) * hourPx.value)
const hourLabels = computed(() =>
  Array.from({ length: hours.last - hours.first }, (_, i) => hours.first + i),
)

function y(day: DayRange, ms: number) {
  return ((ms - day.from) / HOUR - hours.first) * hourPx.value
}

function blockStyle(day: DayRange, b: Block) {
  const activity = b.entry.activityId ? activities.byId.get(b.entry.activityId) : undefined
  const project = b.entry.projectId ? projects.byId.get(b.entry.projectId) : undefined
  const color = activity?.color ?? project?.color
  const top = y(day, b.start)
  return {
    top: `${top}px`,
    height: `${Math.max(y(day, b.end) - top, MIN_BLOCK_PX)}px`,
    left: `calc(${(b.lane / b.lanes) * 100}% + 2px)`,
    width: `calc(${100 / b.lanes}% - 4px)`,
    // Solid fill; text in black or white, whichever reads better. Where the
    // entry continues on another day, the fill fades out at that edge.
    backgroundImage: fill(color ?? 'var(--muted)', continuesBefore(day, b), continuesAfter(day, b)),
    color: color ? textOn(color) : 'var(--foreground)',
  }
}

/** Height of the fade at an edge where a multi-day entry continues. */
const FADE_PX = 18

const continuesBefore = (day: DayRange, b: Block) => b.entry.startAt < day.from
const continuesAfter = (day: DayRange, b: Block) => (b.entry.endAt ?? props.now) > day.to

function fill(color: string, fadeTop: boolean, fadeBottom: boolean): string {
  const stops = [
    ...(fadeTop ? ['transparent 0', `${color} ${FADE_PX}px`] : [`${color} 0`]),
    ...(fadeBottom ? [`${color} calc(100% - ${FADE_PX}px)`, 'transparent 100%'] : [`${color} 100%`]),
  ]
  return `linear-gradient(to bottom, ${stops.join(', ')})`
}

function blockHeight(day: DayRange, b: Block) {
  return y(day, b.end) - y(day, b.start)
}

const activityOf = (e: TimeEntry) => (e.activityId ? activities.byId.get(e.activityId) : undefined)
const projectOf = (e: TimeEntry) => (e.projectId ? projects.byId.get(e.projectId) : undefined)
const tagsOf = (e: TimeEntry) => e.tagIds.map((id) => tags.byId.get(id)).filter((t) => t !== undefined)

/** `09:00–17:00`, with weekdays when the entry spans days (`Mo 22:00–Di 02:00`). */
function timeRange(e: TimeEntry) {
  const end = e.endAt
  if (end !== null && toDateInput(end) !== toDateInput(e.startAt)) {
    return `${format.weekday(e.startAt)} ${format.time(e.startAt)}–${format.weekday(end)} ${format.time(end)}`
  }
  return `${format.time(e.startAt)}–${end === null ? '' : format.time(end)}`
}

function tooltip(e: TimeEntry) {
  const project = projectOf(e)
  return [
    timeRange(e),
    activityOf(e)?.name,
    project ? (project.alias ? `${project.name} (${project.alias})` : project.name) : null,
    tagsOf(e).map((t) => `#${t.name}`).join(' '),
    e.billable ? `€ ${t('entry.billable')}` : null,
    e.note,
  ]
    .filter(Boolean)
    .join('\n')
}

/**
 * Rows a block has room for, by its height: activity (always), project,
 * tags, time, note. Short blocks put the project next to the activity.
 */
function rows(day: DayRange, b: Block) {
  const h = blockHeight(day, b)
  // The time is always shown while dragging this block.
  const dragging = preview.value?.id === b.entry.id
  return { project: h >= 30, tags: h >= 46, time: h >= 62 || dragging, note: h >= 78 }
}

/** Hours shown in the visible area at the current zoom. */
const visibleHours = computed(() => (viewHeight.value - headerHeight.value) / hourPx.value)

/**
 * First visible hour that shows the most entries (at least partly), across
 * all shown days. Ties: more visible tracked time, then a window that
 * includes now, then the earliest. Without entries: an hour before now on
 * today, else 07:00.
 */
function bestStartHour(): number {
  const span = Math.min(24, Math.max(1, visibleHours.value))
  const spans = columns.value.flatMap(({ day, blocks }) =>
    blocks.map((b) => [(b.start - day.from) / HOUR, (b.end - day.from) / HOUR] as const),
  )
  const today = columns.value.find(({ day }) => props.now >= day.from && props.now < day.to)
  const nowHour = today ? (props.now - today.day.from) / HOUR : null
  if (spans.length === 0) return nowHour !== null ? nowHour - 1 : 7

  let best = { start: 0, count: -1, covered: -1, withNow: false }
  // Quarter-hour steps; earlier starts win ties by coming first.
  for (let start = 0; start <= 24 - span + 1e-9; start += 0.25) {
    const end = start + span
    let count = 0
    let covered = 0
    for (const [from, to] of spans) {
      const overlap = Math.min(to, end) - Math.max(from, start)
      if (overlap > 0) {
        count++
        covered += overlap
      }
    }
    const withNow = nowHour !== null && nowHour >= start && nowHour <= end
    const better =
      count > best.count ||
      (count === best.count && covered > best.covered + 1e-9) ||
      (count === best.count && Math.abs(covered - best.covered) <= 1e-9 && withNow && !best.withNow)
    if (better) best = { start, count, covered, withNow }
  }
  return best.start
}

/**
 * Aim the view once per shown range: entries load after the days change,
 * so it re-aims until it has aimed at entries or the user scrolls by hand.
 */
const aimPending = ref(true)

async function scrollToStart() {
  await nextTick()
  if (!scroller.value || viewHeight.value <= 0 || !aimPending.value) return
  scroller.value.scrollTop = Math.max(0, bestStartHour()) * hourPx.value
  // Aimed at real entries: later reloads (e.g. the running timer) keep the position.
  if (props.entries.length) aimPending.value = false
}

function stopAiming() {
  aimPending.value = false
}

onMounted(scrollToStart)
watch(
  () => props.days[0]?.from,
  () => {
    aimPending.value = true
    void scrollToStart()
  },
)
watch(
  () => props.entries,
  () => {
    if (aimPending.value) void scrollToStart()
  },
)
// The zoom is only known once the calendar has a size (0 on mount).
const sized = watch(viewHeight, (h) => {
  if (h > 0) {
    void scrollToStart()
    sized()
  }
})

// In the time format setting (14:00 or 2:00 PM).
function hourLabel(h: number) {
  const day = props.days[0]
  return day ? format.time(fromInputs(toDateInput(day.from), `${String(h).padStart(2, '0')}:00`)) : ''
}
</script>

<template>
  <div ref="scroller" data-calendar class="overflow-auto rounded-lg border" @wheel.passive="stopAiming" @pointerdown="stopAiming" @keydown="stopAiming">
    <div :class="days.length > 1 ? 'min-w-[640px]' : ''">
      <!-- Day headers -->
      <div ref="header" class="sticky top-0 z-10 flex border-b bg-background">
        <div class="w-12 shrink-0" />
        <div
          v-for="(col, i) in columns"
          :key="col.day.from"
          class="flex min-w-0 flex-1 items-baseline justify-between gap-1 border-l px-2 py-1.5 text-xs"
          :class="now >= col.day.from && now < col.day.to ? 'text-primary' : ''"
        >
          <span class="truncate font-medium">
            {{ days.length > 1 ? `${format.weekday(col.day.from)} ${format.shortDate(col.day.from)}` : format.day(col.day.from) }}
          </span>
          <span class="tabular-nums text-muted-foreground">{{ settings.formatDuration(totals[i] ?? 0) }}</span>
        </div>
      </div>

      <!-- Grid -->
      <div class="flex">
        <div class="relative w-12 shrink-0" :style="{ height: `${height}px` }">
          <span
            v-for="h in hourLabels"
            :key="h"
            class="absolute right-1.5 -translate-y-1/2 text-[10px] tabular-nums text-muted-foreground first:translate-y-0"
            :style="{ top: `${(h - hours.first) * hourPx}px` }"
          >
            {{ hourLabel(h) }}
          </span>
        </div>
        <div
          v-for="(col, ci) in columns"
          :key="col.day.from"
          :ref="(el) => { if (el) columnEls[ci] = el as HTMLElement; else columnEls.length = columns.length }"
          class="relative min-w-0 flex-1 overflow-hidden border-l"
          :style="{ height: `${height}px` }"
          @dblclick.self="emit('create', col.day.from)"
        >
          <div
            v-for="h in hourLabels.slice(1)"
            :key="h"
            class="pointer-events-none absolute inset-x-0 border-t border-dashed border-border/70"
            :style="{ top: `${(h - hours.first) * hourPx}px` }"
          />
          <button
            v-for="b in col.blocks"
            :key="b.entry.id"
            type="button"
            class="group/block absolute flex flex-col gap-0.5 overflow-hidden rounded-md px-1.5 py-0.5 text-left text-[11px] leading-tight shadow-xs transition hover:brightness-95 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none dark:hover:brightness-125"
            :class="[
              b.entry.endAt === null ? 'ring-1 ring-green-500/60' : '',
              preview?.id === b.entry.id ? 'z-[2] shadow-lg ring-2 ring-ring' : '',
              // Where the entry continues on another day: faded, square edge,
              // no shadow; content starts below the top fade.
              continuesBefore(col.day, b) ? 'rounded-t-none pt-3 shadow-none' : '',
              continuesAfter(col.day, b) ? 'rounded-b-none shadow-none' : '',
            ]"
            :style="blockStyle(col.day, b)"
            :title="preview ? undefined : tooltip(b.entry)"
            @click="onBlockClick(b.entry, $event)"
          >
            <!-- Drag handles on the top and bottom edge -->
            <span
              v-if="canResize(col.day, b, 'start')"
              class="absolute inset-x-0 top-0 z-[1] flex h-2 cursor-ns-resize touch-none justify-center"
              :aria-label="$t('timesheet.dragStart')"
              @pointerdown.stop.prevent="startDrag($event, b, 'start')"
            ><span class="mt-0.5 h-0.5 w-6 rounded-full bg-current opacity-0 transition-opacity group-hover/block:opacity-50" /></span>
            <span
              v-if="canResize(col.day, b, 'end')"
              class="absolute inset-x-0 bottom-0 z-[1] flex h-2 cursor-ns-resize touch-none items-end justify-center"
              :aria-label="$t('timesheet.dragEnd')"
              @pointerdown.stop.prevent="startDrag($event, b, 'end')"
            ><span class="mb-0.5 h-0.5 w-6 rounded-full bg-current opacity-0 transition-opacity group-hover/block:opacity-50" /></span>
            <!-- Activity, billable -->
            <span class="flex min-w-0 items-center gap-1">
              <span class="truncate font-medium">{{ activityOf(b.entry)?.name ?? $t('activities.none') }}</span>
              <span
                v-if="!rows(col.day, b).project && projectOf(b.entry)"
                class="truncate opacity-75"
              >· {{ projectOf(b.entry)!.alias ?? projectOf(b.entry)!.name }}</span>
              <span
                v-if="b.entry.billable"
                class="ml-auto shrink-0 rounded-sm bg-background/90 px-1 text-[10px] font-semibold text-foreground"
                :title="$t('entry.billable')"
              >€</span>
            </span>
            <!-- Project -->
            <!-- Project, tags and billable sit on neutral chips, so they stay
                 readable whatever the activity's color is. The project is a
                 square chip, tags are `#` pills. -->
            <span
              v-if="rows(col.day, b).project && projectOf(b.entry)"
              class="flex w-fit max-w-full min-w-0 items-center gap-1 rounded-sm bg-background/90 px-1.5 py-0.5 font-medium text-foreground"
            >
              <span class="truncate">{{ projectOf(b.entry)!.name }}</span>
              <span v-if="projectOf(b.entry)!.alias" class="shrink-0 font-mono text-[10px] text-muted-foreground">{{ projectOf(b.entry)!.alias }}</span>
            </span>
            <!-- Tags: one line, the rest is cut off (all are in the tooltip) -->
            <span v-if="rows(col.day, b).tags && b.entry.tagIds.length" class="flex min-w-0 gap-1 overflow-hidden">
              <span
                v-for="tag in tagsOf(b.entry)"
                :key="tag.id"
                class="inline-flex shrink-0 items-center gap-1 rounded-full bg-background/90 py-px pr-1.5 pl-1 text-[10px] leading-4 text-foreground"
              >
                <span class="size-1.5 rounded-full ring-1 ring-foreground/20" :style="{ backgroundColor: tag.color }" />
                <span><span class="text-muted-foreground">#</span>{{ tag.name }}</span>
              </span>
            </span>
            <span v-if="rows(col.day, b).time" class="truncate tabular-nums opacity-75">{{ timeRange(shownEntry(b.entry)) }}</span>
            <span v-if="rows(col.day, b).note && b.entry.note" class="truncate opacity-75">{{ b.entry.note }}</span>
          </button>
          <div
            v-if="now >= col.day.from && now < col.day.to"
            class="pointer-events-none absolute inset-x-0 z-[1] border-t-2 border-red-500"
            :style="{ top: `${y(col.day, now)}px` }"
          />
        </div>
      </div>
    </div>
  </div>
</template>
