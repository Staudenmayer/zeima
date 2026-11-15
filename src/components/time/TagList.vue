<script setup lang="ts">
import { computed } from 'vue'
import { useTagsStore } from '@/stores/tags'

const props = defineProps<{ ids: string[] }>()
const tags = useTagsStore()
const list = computed(() => props.ids.map((id) => tags.byId.get(id)).filter((t) => t !== undefined))
</script>

<template>
  <span class="inline-flex flex-nowrap gap-1 whitespace-nowrap">
    <span
      v-for="tag in list"
      :key="tag.id"
      class="shrink-0 rounded-full px-2 py-px text-[11px] leading-4 text-white"
      :style="{ backgroundColor: tag.color }"
    >{{ tag.name }}</span>
  </span>
</template>
