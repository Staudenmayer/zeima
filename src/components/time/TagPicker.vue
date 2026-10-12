<script setup lang="ts">
import { computed, watch } from 'vue'
import type { Tag } from '@/bindings'
import { useTagsStore } from '@/stores/tags'

const props = defineProps<{ projectId: string | null }>()
const selected = defineModel<string[]>({ required: true })

const tags = useTagsStore()
const usable = computed(() => tags.usableFor(props.projectId))

// Drop project tags that no longer apply after switching the project.
watch(usable, (list) => {
  const ids = new Set(list.map((t) => t.id))
  if (selected.value.some((id) => !ids.has(id))) {
    selected.value = selected.value.filter((id) => ids.has(id))
  }
})

/** Selecting a tag deselects others of the same group. */
function toggle(tag: Tag) {
  if (selected.value.includes(tag.id)) {
    selected.value = selected.value.filter((id) => id !== tag.id)
    return
  }
  const sameGroup = (id: string) => tag.groupName !== null && tags.byId.get(id)?.groupName === tag.groupName
  selected.value = [...selected.value.filter((id) => !sameGroup(id)), tag.id]
}
</script>

<template>
  <div class="flex flex-wrap gap-1.5">
    <button
      v-for="tag in usable"
      :key="tag.id"
      type="button"
      class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs transition-colors"
      :class="selected.includes(tag.id) ? 'border-transparent text-white' : 'text-muted-foreground hover:bg-muted'"
      :style="selected.includes(tag.id) ? { backgroundColor: tag.color } : undefined"
      @click="toggle(tag)"
    >
      <span v-if="!selected.includes(tag.id)" class="size-2 rounded-full" :style="{ backgroundColor: tag.color }" />
      {{ tag.name }}
      <span v-if="tag.groupName" class="opacity-70">· {{ tag.groupName }}</span>
    </button>
    <span v-if="usable.length === 0" class="text-sm text-muted-foreground">{{ $t('common.none') }}</span>
  </div>
</template>
