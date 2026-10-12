<script setup lang="ts">
import type { LucideIcon } from '@lucide/vue'
import { useRoute } from 'vue-router'
import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'

defineProps<{
  label?: string
  items: {
    title: string
    url: string
    icon: LucideIcon
  }[]
}>()

const route = useRoute()

const isActive = (url: string) => (url === '/' ? route.path === '/' : route.path.startsWith(url))
</script>

<template>
  <SidebarGroup>
    <SidebarGroupLabel v-if="label">{{ label }}</SidebarGroupLabel>
    <SidebarMenu>
      <SidebarMenuItem v-for="item in items" :key="item.url">
        <SidebarMenuButton as-child :tooltip="item.title" :is-active="isActive(item.url)">
          <RouterLink :to="item.url">
            <component :is="item.icon" />
            <span>{{ item.title }}</span>
          </RouterLink>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  </SidebarGroup>
</template>
