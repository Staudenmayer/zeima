<script setup lang="ts">
import type { SidebarProps } from '@/components/ui/sidebar'

import { computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAccountStore } from '@/stores/account'
import {
  BarChart3,
  Clock,
  FolderKanban,
  Building2,
  Landmark,
  LayoutDashboard,
  Palmtree,
  Puzzle,
  Settings,
  Shapes,
  Tags,
} from '@lucide/vue'
import LogoMark from '@/components/LogoMark.vue'
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
const account = useAccountStore()
onMounted(async () => {
  await account.load()
  await account.loadSync()
})

const tracking = computed(() => [
  { title: t('nav.dashboard'), url: '/', icon: LayoutDashboard },
  { title: t('nav.timesheet'), url: '/timesheet', icon: Clock },
  { title: t('nav.activities'), url: '/activities', icon: Shapes },
  { title: t('nav.projects'), url: '/projects', icon: FolderKanban },
  { title: t('nav.tags'), url: '/tags', icon: Tags },
  { title: t('nav.costCenters'), url: '/cost-centers', icon: Landmark },
  { title: t('nav.reports'), url: '/reports', icon: BarChart3 },
  { title: t('nav.absences'), url: '/absences', icon: Palmtree },
])

const system = computed(() => [
  ...(account.status.signedIn ? [{ title: t('nav.organization'), url: '/organization', icon: Building2 }] : []),
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
              <div class="flex aspect-square size-8 items-center justify-center rounded-lg bg-blue-600">
                <LogoMark on-blue class="size-5" />
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
      <div class="px-2 pb-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
        <p class="truncate">
          {{ account.status.signedIn ? $t('account.signedInAs', { name: account.status.user?.email ?? account.status.user?.name ?? '' }) : $t('app.local') }}
        </p>
        <RouterLink
          v-if="account.syncSummary"
          to="/settings"
          class="block truncate hover:underline"
          :class="account.syncSummary.problem ? 'text-amber-600 dark:text-amber-400' : ''"
        >
          {{ $t(account.syncSummary.key, account.syncSummary.params ?? {}) }}
        </RouterLink>
      </div>
    </SidebarFooter>
    <SidebarRail />
  </Sidebar>
</template>
