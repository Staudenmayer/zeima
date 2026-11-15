<script setup lang="ts">
import { useRoute } from 'vue-router'
import { useNav } from '@/composables/useNav'

const route = useRoute()
const { tabs } = useNav()
</script>

<template>
  <nav v-if="tabs.length > 1" :aria-label="$t('nav.section')" class="mb-4 inline-flex h-9 w-fit items-center rounded-lg bg-muted p-[3px] text-muted-foreground">
    <RouterLink
      v-for="tab in tabs"
      :key="tab.url"
      :to="tab.url"
      class="inline-flex h-[calc(100%-1px)] items-center rounded-md px-3 text-sm font-medium whitespace-nowrap"
      :class="route.path.startsWith(tab.url) ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground'"
    >{{ tab.title }}</RouterLink>
  </nav>
</template>
