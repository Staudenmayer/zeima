<script setup lang="ts">
import { useRoute } from 'vue-router'

const route = useRoute()
const tabs = [
  { to: '/settings', label: 'settings.tabs.general' },
  { to: '/settings/work-hours', label: 'settings.tabs.workHours' },
  { to: '/settings/auto-pause', label: 'settings.tabs.autoPause' },
  { to: '/settings/data', label: 'settings.tabs.data' },
]
</script>

<template>
  <nav class="mb-6 inline-flex rounded-lg bg-muted p-1 text-sm">
    <RouterLink
      v-for="tab in tabs"
      :key="tab.to"
      :to="tab.to"
      class="rounded-md px-3 py-1.5 text-muted-foreground transition-colors"
      :class="route.path === tab.to ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground'"
    >
      {{ $t(tab.label) }}
    </RouterLink>
  </nav>
</template>
