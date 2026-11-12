<script lang="ts">
export type DonutSlice = { key: string; label: string; color: string; ms: number }
</script>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  slices: DonutSlice[]
  /** Accessible name of the chart. */
  label: string
  formatDuration: (ms: number) => string
}>()

const total = computed(() => props.slices.reduce((sum, s) => sum + s.ms, 0) || 1)

const rows = computed(() =>
  props.slices.map((s) => ({ ...s, pct: Math.round((s.ms / total.value) * 100) })),
)

// One conic-gradient stop per slice; the hole is a radial mask.
const gradient = computed(() => {
  let acc = 0
  const stops = props.slices.map((s) => {
    const from = (acc / total.value) * 100
    acc += s.ms
    return `${s.color} ${from}% ${(acc / total.value) * 100}%`
  })
  return `conic-gradient(${stops.join(', ')})`
})
</script>

<template>
  <div class="grid gap-5">
    <div
      role="img"
      :aria-label="label"
      class="mx-auto size-33 rounded-full [mask:radial-gradient(circle,transparent_38px,#000_39px)]"
      :style="{ background: gradient }"
    />
    <ul class="grid gap-2 text-sm">
      <li v-for="row in rows" :key="row.key" class="flex items-center justify-between gap-2">
        <span class="flex min-w-0 items-center gap-2">
          <span class="size-2.5 shrink-0 rounded-full" :style="{ backgroundColor: row.color }" />
          <span class="truncate">{{ row.label }}</span>
        </span>
        <span class="shrink-0 tabular-nums text-muted-foreground">{{ formatDuration(row.ms) }} h · {{ row.pct }} %</span>
      </li>
    </ul>
  </div>
</template>
