<script setup lang="ts">
import { computed } from 'vue'
import type { TimeEntry } from '@/bindings'
import { useFormat } from '@/composables/useFormat'
import { HOUR, fromInputs, toDateInput, type DayRange } from '@/lib/time'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'

// Day/week calendar grid: one column per day, entries as blocks positioned
// by time. Overlapping entries share the column side by side.

const props = defineProps<{
  days: DayRange[]
  entries: TimeEntry[]
  /** Day totals in ms, same order as `days`. */
  totals: number[]
  now: number
}>()

const emit = defineEmits<{
  edit: [entry: TimeEntry]
  /** Double-click on an empty slot: start of that day. */
  create: [day: number]
}>()

const projects = useProjectsStore()
const settings = useSettingsStore()
const format = useFormat()

const HOUR_PX = 44
const MIN_BLOCK_PX = 18

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

/** Visible hours: at least 07–19, widened to fit every entry. */
const hours = computed(() => {
  let first = 7
  let last = 19
  for (const { day, blocks } of columns.value) {
    for (const b of blocks) {
      first = Math.min(first, Math.floor((b.start - day.from) / HOUR))
      last = Math.max(last, Math.ceil((b.end - day.from) / HOUR))
    }
  }
  return { first, last: Math.min(last, 24) }
})

const height = computed(() => (hours.value.last - hours.value.first) * HOUR_PX)
const hourLabels = computed(() =>
  Array.from({ length: hours.value.last - hours.value.first }, (_, i) => hours.value.first + i),
)

function y(day: DayRange, ms: number) {
  return ((ms - day.from) / HOUR - hours.value.first) * HOUR_PX
}

function blockStyle(day: DayRange, b: Block) {
  const color = b.entry.projectId ? projects.byId.get(b.entry.projectId)?.color : undefined
  const top = y(day, b.start)
  return {
    top: `${top}px`,
    height: `${Math.max(y(day, b.end) - top, MIN_BLOCK_PX)}px`,
    left: `calc(${(b.lane / b.lanes) * 100}% + 2px)`,
    width: `calc(${100 / b.lanes}% - 4px)`,
    backgroundColor: color ? `color-mix(in oklab, ${color} 20%, var(--background))` : 'var(--muted)',
    borderLeftColor: color ?? 'var(--muted-foreground)',
  }
}

function blockHeight(day: DayRange, b: Block) {
  return y(day, b.end) - y(day, b.start)
}

function label(entry: TimeEntry) {
  const project = entry.projectId ? projects.byId.get(entry.projectId) : null
  return project ? (project.alias ?? project.name) : ''
}

function tooltip(entry: TimeEntry) {
  const project = entry.projectId ? projects.byId.get(entry.projectId) : null
  const time = `${format.time(entry.startAt)}–${entry.endAt === null ? '' : format.time(entry.endAt)}`
  return [time, project?.name, entry.note].filter(Boolean).join('\n')
}

// In the time format setting (14:00 or 2:00 PM).
function hourLabel(h: number) {
  const day = props.days[0]
  return day ? format.time(fromInputs(toDateInput(day.from), `${String(h).padStart(2, '0')}:00`)) : ''
}
</script>

<template>
  <div class="overflow-x-auto rounded-lg border">
    <div :class="days.length > 1 ? 'min-w-[640px]' : ''">
      <!-- Day headers -->
      <div class="sticky top-0 z-10 flex border-b bg-background">
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
            :style="{ top: `${(h - hours.first) * HOUR_PX}px` }"
          >
            {{ hourLabel(h) }}
          </span>
        </div>
        <div
          v-for="col in columns"
          :key="col.day.from"
          class="relative min-w-0 flex-1 overflow-hidden border-l"
          :style="{ height: `${height}px` }"
          @dblclick.self="emit('create', col.day.from)"
        >
          <div
            v-for="h in hourLabels.slice(1)"
            :key="h"
            class="pointer-events-none absolute inset-x-0 border-t border-dashed border-border/70"
            :style="{ top: `${(h - hours.first) * HOUR_PX}px` }"
          />
          <button
            v-for="b in col.blocks"
            :key="b.entry.id"
            type="button"
            class="absolute flex flex-col overflow-hidden rounded-md border-l-[3px] px-1.5 py-0.5 text-left text-[11px] leading-tight shadow-xs transition hover:brightness-95 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none dark:hover:brightness-125"
            :class="b.entry.endAt === null ? 'ring-1 ring-green-500/60' : ''"
            :style="blockStyle(col.day, b)"
            :title="tooltip(b.entry)"
            @click="emit('edit', b.entry)"
          >
            <span class="truncate font-medium">
              {{ label(b.entry) || b.entry.note || $t('common.noProject') }}
            </span>
            <span v-if="blockHeight(col.day, b) >= 30" class="truncate tabular-nums text-muted-foreground">
              {{ format.time(b.entry.startAt) }}–{{ b.entry.endAt === null ? '' : format.time(b.entry.endAt) }}
            </span>
            <span v-if="blockHeight(col.day, b) >= 44 && label(b.entry) && b.entry.note" class="truncate text-muted-foreground">
              {{ b.entry.note }}
            </span>
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
