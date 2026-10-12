<script setup lang="ts">
import type { SidebarProps } from '@/components/ui/sidebar'

import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  BarChart3,
  Clock,
  FolderKanban,
  Landmark,
  LayoutDashboard,
  Palmtree,
  Puzzle,
  Settings,
  Tags,
  Timer,
} from '@lucide/vue'
import NavMain from '@/components/NavMain.vue'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from '@/components/ui/sidebar'
//import NavUser from '@/components/NavUser.vue'

const props = withDefaults(defineProps<SidebarProps>(), {
  collapsible: 'icon',
})

const { t } = useI18n()

const tracking = computed(() => [
  { title: t('nav.dashboard'), url: '/', icon: LayoutDashboard },
  { title: t('nav.timesheet'), url: '/timesheet', icon: Clock },
  { title: t('nav.projects'), url: '/projects', icon: FolderKanban },
  { title: t('nav.tags'), url: '/tags', icon: Tags },
  { title: t('nav.costCenters'), url: '/cost-centers', icon: Landmark },
  { title: t('nav.reports'), url: '/reports', icon: BarChart3 },
  { title: t('nav.absences'), url: '/absences', icon: Palmtree },
])

const system = computed(() => [
  { title: t('nav.addons'), url: '/addons', icon: Puzzle },
  { title: t('nav.settings'), url: '/settings', icon: Settings },
])

/*
const user = computed(() => ({
  name: 'shadcn',
  email: 'me@example.com',
  avatar: '/avatars/shadcn.jpg'
}))
*/
</script>

<template>
  <Sidebar v-bind="props">
    <SidebarHeader>
      <SidebarMenu>
        <SidebarMenuItem>
          <SidebarMenuButton size="lg" as-child>
            <RouterLink to="/">
              <div class="flex aspect-square size-8 items-center justify-center rounded-lg bg-sidebar-primary text-sidebar-primary-foreground">
                <Timer class="size-4" />
              </div>
              <div class="grid flex-1 text-left text-sm leading-tight">
                <span class="truncate font-semibold">{{ $t('app.name') }}</span>
                <span class="truncate text-xs text-muted-foreground">{{ $t('app.personal') }}</span>
              </div>
            </RouterLink>
          </SidebarMenuButton>
        </SidebarMenuItem>
      </SidebarMenu>
    </SidebarHeader>
    <SidebarContent>
      <NavMain :items="tracking" />
    </SidebarContent>
    <SidebarFooter>
      <!--<NavUser :user="user"></NavUser>-->
      <NavMain :items="system" />
      <p class="px-2 pb-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">{{ $t('app.local') }}</p>
    </SidebarFooter>
    <SidebarRail />
  </Sidebar>
</template>
