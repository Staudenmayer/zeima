<script setup lang="ts">
import type { LucideIcon } from '@lucide/vue'
import { ChevronsUpDown, LogOut } from '@lucide/vue'
import { computed } from 'vue'
import { toast } from 'vue-sonner'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from '@/components/ui/sidebar'
import { errorMessage } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'

// The account at the bottom of the sidebar: who is signed in and how the sync
// is doing; its menu holds the account, the system pages and "Sign out".
defineProps<{ items: { title: string; url: string; icon: LucideIcon }[] }>()

const account = useAccountStore()
const { isMobile } = useSidebar()

const email = computed(() => account.status.user?.email ?? account.status.user?.name ?? '')
const name = computed(() => (account.status.signedIn ? (account.status.user?.name ?? email.value.split('@')[0] ?? '') : null))
const initial = computed(() => (name.value || 'L').charAt(0).toUpperCase())
const problem = computed(() => account.syncSummary?.problem ?? false)

async function signOut() {
  try {
    await account.signOut()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <SidebarMenu>
    <SidebarMenuItem>
      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <SidebarMenuButton
            size="lg"
            :aria-label="$t('account.menu')"
            class="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
          >
            <Avatar class="size-8 rounded-lg">
              <AvatarFallback class="rounded-lg font-semibold">{{ initial }}</AvatarFallback>
            </Avatar>
            <div class="grid flex-1 text-left text-sm leading-tight">
              <span class="truncate font-semibold">{{ name ?? $t('app.localUser') }}</span>
              <span class="truncate text-xs" :class="problem ? 'text-amber-600 dark:text-amber-400' : 'text-muted-foreground'">
                {{ account.syncSummary ? $t(account.syncSummary.key, account.syncSummary.params ?? {}) : $t('account.logInToSync') }}
              </span>
            </div>
            <ChevronsUpDown class="ml-auto size-4 opacity-60" />
          </SidebarMenuButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          class="w-(--reka-dropdown-menu-trigger-width) min-w-64 rounded-lg"
          :side="isMobile ? 'bottom' : 'right'"
          align="end"
          :side-offset="4"
        >
          <!-- The account itself: opens Account & sync in the settings. -->
          <DropdownMenuItem as-child class="gap-2 py-2">
            <RouterLink to="/settings">
              <Avatar class="size-8 rounded-lg">
                <AvatarFallback class="rounded-lg font-semibold">{{ initial }}</AvatarFallback>
              </Avatar>
              <div class="grid flex-1 text-left text-sm leading-tight">
                <span class="truncate font-semibold">{{ name ?? $t('app.localUser') }}</span>
                <span class="truncate text-xs text-muted-foreground">{{ account.status.signedIn ? email : $t('account.localSub') }}</span>
              </div>
              <span class="shrink-0 text-xs text-muted-foreground">{{ account.status.signedIn ? $t('account.accountAndSync') : $t('account.logIn') }}</span>
            </RouterLink>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem v-for="item in items" :key="item.url" as-child>
            <RouterLink :to="item.url"><component :is="item.icon" /> {{ item.title }}</RouterLink>
          </DropdownMenuItem>
          <template v-if="account.status.signedIn">
            <DropdownMenuSeparator />
            <DropdownMenuItem @click="signOut"><LogOut /> {{ $t('account.signOut') }}</DropdownMenuItem>
          </template>
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  </SidebarMenu>
</template>
