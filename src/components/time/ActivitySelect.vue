<script setup lang="ts">
import { computed } from 'vue'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useActivitiesStore } from '@/stores/activities'
import ActivityLabel from './ActivityLabel.vue'

const props = defineProps<{ invalid?: boolean }>()
const model = defineModel<string | null>({ required: true })

const activities = useActivitiesStore()
// Archived activities are not offered, but the current one stays visible.
const options = computed(() => activities.all.filter((a) => !a.archived || a.id === model.value))
const value = computed({
  get: () => model.value ?? undefined,
  set: (v: string | undefined) => (model.value = v ?? null),
})
</script>

<template>
  <Select v-model="value">
    <SelectTrigger class="w-full min-w-0" :aria-invalid="props.invalid || undefined">
      <SelectValue :placeholder="$t('activities.choose')">
        <ActivityLabel v-if="model" :activity="activities.byId.get(model)" />
      </SelectValue>
    </SelectTrigger>
    <SelectContent>
      <SelectItem v-for="a in options" :key="a.id" :value="a.id">
        <ActivityLabel :activity="a" />
      </SelectItem>
    </SelectContent>
  </Select>
</template>
