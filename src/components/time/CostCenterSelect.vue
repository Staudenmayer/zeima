<script setup lang="ts">
import { computed } from 'vue'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { costCenterLabel, useCostCentersStore } from '@/stores/costCenters'

const props = defineProps<{
  /** Label of the empty choice, e.g. "None" or "Automatic". */
  emptyLabel: string
  /** Ids that must not be offered (e.g. the cost center itself as parent). */
  exclude?: string[]
}>()
const model = defineModel<string | null>({ required: true })

// Select items cannot have an empty value.
const NONE = '__none__'
const costCenters = useCostCentersStore()
// Inactive ones stay selectable when already chosen (old entries).
const options = computed(() =>
  costCenters.all.filter((c) => (c.active || c.id === model.value) && !props.exclude?.includes(c.id)),
)
const value = computed({
  get: () => model.value ?? NONE,
  set: (v: string) => (model.value = v === NONE ? null : v),
})
</script>

<template>
  <Select v-model="value">
    <SelectTrigger class="w-full">
      <SelectValue>
        <span v-if="model" class="truncate">{{ costCenterLabel(costCenters.byId.get(model)) }}</span>
        <span v-else class="text-muted-foreground">{{ emptyLabel }}</span>
      </SelectValue>
    </SelectTrigger>
    <SelectContent>
      <SelectItem :value="NONE">
        <span class="text-muted-foreground">{{ emptyLabel }}</span>
      </SelectItem>
      <SelectItem v-for="c in options" :key="c.id" :value="c.id">
        <span class="font-mono text-xs">{{ c.code }}</span> {{ c.name }}
      </SelectItem>
    </SelectContent>
  </Select>
</template>
