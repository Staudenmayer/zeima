<script setup lang="ts">
import { computed } from 'vue'
import { useTagsStore } from '@/stores/tags'

const props = defineProps<{ ids: string[] }>()
const tags = useTagsStore()
const list = computed(() => props.ids.map((id) => tags.byId.get(id)).filter((t) => t !== undefined))
</script>

<template>
  <span class="inline-flex flex-wrap gap-1">
    <span
      v-for="tag in list"
      :key="tag.id"
      class="rounded-full px-2 py-px text-[11px] leading-4 text-white"
      :style="{ backgroundColor: tag.color }"
    >{{ tag.name }}</span>
  </span>
</template>
