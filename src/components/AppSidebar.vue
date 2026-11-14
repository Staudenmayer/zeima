<script setup lang="ts">
import type { SidebarProps } from '@/components/ui/sidebar'

import { computed, onMounted, watch } from 'vue'
import { toast } from 'vue-sonner'
import { useI18n } from 'vue-i18n'
import { useAccountStore } from '@/stores/account'
import { useApprovalsStore } from '@/stores/approvals'
import { useWorkspaceStore } from '@/stores/workspace'
import {
  BarChart3,
  Clock,
  FolderKanban,
  Building2,
  Check,
  ChevronsUpDown,
  User,
  ClipboardCheck,
  Users,
  CalendarDays,
  MapPinned,
  Landmark,
  LayoutDashboard,
  Palmtree,
  Puzzle,
  Settings,
  Shapes,
  Tags,
} from '@lucide/vue'
import LogoMark from '@/components/LogoMark.vue'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { errorMessage } from '@/lib/ipc'
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
import NavUser from '@/components/NavUser.vue'

const props = withDefaults(defineProps<SidebarProps>(), {
  collapsible: 'icon',
})

const { t } = useI18n()
const account = useAccountStore()
const workspaces = useWorkspaceStore()
const approvals = useApprovalsStore()
onMounted(async () => {
  await account.load()
  await account.loadSync()
  await workspaces.load().catch(() => {})
})
// A sync may have brought in an organization's workspace.
watch(() => account.sync?.lastSyncedAt, () => void workspaces.load().catch(() => {}))
// What waits for the person's decision shows next to the Approvals entry.
watch(
  () => [workspaces.active?.id, account.sync?.lastSyncedAt],
  () => {
    if (workspaces.inOrg) void approvals.load().catch(() => {})
  },
  { immediate: true },
)

const tracking = computed(() => [
  { title: t('nav.dashboard'), url: '/', icon: LayoutDashboard },
  { title: t('nav.timesheet'), url: '/timesheet', icon: Clock },
  { title: t('nav.activities'), url: '/activities', icon: Shapes },
  { title: t('nav.projects'), url: '/projects', icon: FolderKanban },
  { title: t('nav.tags'), url: '/tags', icon: Tags },
  { title: t('nav.costCenters'), url: '/cost-centers', icon: Landmark },
  { title: t('nav.reports'), url: '/reports', icon: BarChart3 },
  { title: t('nav.absences'), url: '/absences', icon: Palmtree },
  ...(workspaces.inOrg ? [{ title: approvals.waiting ? `${t('nav.approvals')} (${approvals.waiting})` : t('nav.approvals'), url: '/approvals', icon: ClipboardCheck }] : []),
  ...(workspaces.inOrg ? [{ title: t('nav.meetings'), url: '/meetings', icon: CalendarDays }] : []),
  ...(workspaces.inOrg ? [{ title: t('nav.locations'), url: '/locations', icon: MapPinned }] : []),
  // Owners and managers see other people's time; for the rest the page would show only themselves.
  ...(workspaces.inOrg && (workspaces.role === 'owner' || workspaces.role === 'manager')
    ? [{ title: t('nav.team'), url: '/team', icon: Users }]
    : []),
])

const system = computed(() => [
  ...(account.status.signedIn ? [{ title: t('nav.organization'), url: '/organization', icon: Building2 }] : []),
  { title: t('nav.addons'), url: '/addons', icon: Puzzle },
  { title: t('nav.settings'), url: '/settings', icon: Settings },
])

async function pickWorkspace(id: string) {
  try {
    await workspaces.switchTo(id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <Sidebar v-bind="props">
    <SidebarHeader>
      <SidebarMenu>
        <SidebarMenuItem>
          <!-- With more than one workspace the brand is the workspace switcher. -->
          <DropdownMenu v-if="workspaces.switchable">
            <DropdownMenuTrigger as-child>
              <SidebarMenuButton size="lg" :aria-label="$t('app.switchWorkspace')" class="data-[state=open]:bg-sidebar-accent">
                <div class="flex aspect-square size-8 items-center justify-center rounded-lg bg-blue-600">
                  <LogoMark on-blue class="size-5" />
                </div>
                <div class="grid flex-1 text-left text-sm leading-tight">
                  <span class="truncate font-semibold">{{ $t('app.name') }}</span>
                  <span class="truncate text-xs text-muted-foreground">{{ workspaces.inOrg ? workspaces.active?.name : $t('app.personal') }}</span>
                </div>
                <ChevronsUpDown class="ml-auto size-4 opacity-60" />
              </SidebarMenuButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" class="w-(--reka-dropdown-menu-trigger-width) min-w-56">
              <DropdownMenuItem v-for="w in workspaces.all" :key="w.id" class="gap-2" @click="pickWorkspace(w.id)">
                <User v-if="w.kind === 'personal'" class="size-4" />
                <Building2 v-else class="size-4" />
                <span class="flex-1 truncate">{{ w.kind === 'personal' ? $t('app.personal') : w.name }}</span>
                <Check v-if="w.active" class="size-4" />
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <SidebarMenuButton v-else size="lg" as-child>
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
      <NavUser :items="system" />
    </SidebarFooter>
    <SidebarRail />
  </Sidebar>
</template>
