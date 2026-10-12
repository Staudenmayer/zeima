<script setup lang="ts">
import { computed } from 'vue'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useProjectsStore } from '@/stores/projects'
import ProjectDot from './ProjectDot.vue'

const props = defineProps<{
  /** Project ids that must not be offered (e.g. the project itself as parent). */
  exclude?: string[]
  /** Offer archived projects too (when editing old entries). */
  includeArchived?: boolean
}>()
const model = defineModel<string | null>({ required: true })

// Select items cannot have an empty value.
const NONE = '__none__'
const projects = useProjectsStore()
const options = computed(() =>
  (props.includeArchived ? projects.all : projects.active).filter((p) => !props.exclude?.includes(p.id)),
)
const value = computed({
  get: () => model.value ?? NONE,
  set: (v: string) => (model.value = v === NONE ? null : v),
})
</script>

<template>
  <Select v-model="value">
    <SelectTrigger class="w-full min-w-0">
      <SelectValue>
        <ProjectDot :project="model ? projects.byId.get(model) : null" />
      </SelectValue>
    </SelectTrigger>
    <SelectContent>
      <SelectItem :value="NONE">
        <ProjectDot :project="null" />
      </SelectItem>
      <SelectItem v-for="p in options" :key="p.id" :value="p.id">
        <ProjectDot :project="p" show-alias />
      </SelectItem>
    </SelectContent>
  </Select>
</template>
