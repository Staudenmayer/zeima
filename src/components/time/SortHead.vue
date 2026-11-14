<script setup lang="ts">
import { computed } from 'vue'
import { ArrowDown, ArrowUp, ArrowUpDown } from '@lucide/vue'
import type { SortState } from '@/composables/useSort'

// A table header cell that sorts its column when clicked (see `useSort`).
// Classes on it (width, alignment) go on the `th`.
const props = defineProps<{ sort: SortState; column: string }>()

const active = computed(() => props.sort.key === props.column)
const ariaSort = computed(() => (active.value ? (props.sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'))
</script>

<template>
  <th :aria-sort="ariaSort">
    <button
      type="button"
      class="-mx-1 inline-flex items-center gap-1 rounded px-1 py-0.5 font-medium whitespace-nowrap hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      :class="active ? 'text-foreground' : ''"
      @click="sort.toggle(column)"
    >
      <slot />
      <ArrowUp v-if="active && sort.dir === 'asc'" class="size-3" />
      <ArrowDown v-else-if="active" class="size-3" />
      <ArrowUpDown v-else class="size-3 opacity-30" />
    </button>
  </th>
</template>
