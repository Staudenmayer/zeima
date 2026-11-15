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
    members?: { url: string }[]
  }[]
}>()

const route = useRoute()

const isActive = (url: string) => (url === '/' ? route.path === '/' : route.path.startsWith(url))
const isOn = (item: { url: string; members?: { url: string }[] }) =>
  item.members ? item.members.some((m) => isActive(m.url)) : isActive(item.url)
// A group opens the page that was last open, else its first one.
const target = (item: { url: string; members?: { url: string }[] }) =>
  item.members?.find((m) => isActive(m.url))?.url ?? item.url
</script>

<template>
  <SidebarGroup>
    <SidebarGroupLabel v-if="label">{{ label }}</SidebarGroupLabel>
    <SidebarMenu>
      <SidebarMenuItem v-for="item in items" :key="item.url">
        <SidebarMenuButton as-child :tooltip="item.title" :is-active="isOn(item)">
          <RouterLink :to="target(item)">
            <component :is="item.icon" />
            <span>{{ item.title }}</span>
          </RouterLink>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  </SidebarGroup>
</template>
