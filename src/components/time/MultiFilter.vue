<script setup lang="ts">
import { computed } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

export interface FilterOption {
  /** `null` stands for "without" (e.g. no project). */
  value: string | null
  label: string
  color?: string
}

const props = defineProps<{ label: string; options: FilterOption[] }>()
/** Selected values; empty = no filter. */
const model = defineModel<(string | null)[]>({ required: true })

const summary = computed(() => {
  if (model.value.length === 0) return props.label
  if (model.value.length === 1) {
    return props.options.find((o) => o.value === model.value[0])?.label ?? props.label
  }
  return `${props.label} · ${model.value.length}`
})

function toggle(value: string | null, on: boolean) {
  model.value = on ? [...model.value, value] : model.value.filter((v) => v !== value)
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="outline" size="sm" :class="model.length ? 'border-primary/60' : ''">
        <span class="max-w-40 truncate">{{ summary }}</span>
        <ChevronDown class="opacity-50" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="start" class="max-h-80 min-w-56 overflow-y-auto">
      <DropdownMenuCheckboxItem
        v-for="o in options"
        :key="o.value ?? '__none__'"
        :model-value="model.includes(o.value)"
        @update:model-value="toggle(o.value, $event)"
        @select.prevent
      >
        <span v-if="o.color" class="size-2.5 shrink-0 rounded-full" :style="{ backgroundColor: o.color }" />
        <span class="truncate" :class="o.value === null ? 'text-muted-foreground' : ''">{{ o.label }}</span>
      </DropdownMenuCheckboxItem>
      <template v-if="model.length">
        <DropdownMenuSeparator />
        <DropdownMenuItem @select="model = []">{{ $t('reports.clearFilter') }}</DropdownMenuItem>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
