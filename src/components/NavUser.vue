<script setup lang="ts">
import type { LucideIcon } from '@lucide/vue'
import { ChevronsUpDown, LogOut } from '@lucide/vue'
import { computed, onBeforeUnmount, onMounted, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
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
import PresenceDot from '@/components/time/PresenceDot.vue'
import { events } from '@/bindings'
import { useOrgStore } from '@/stores/org'

// The account at the bottom of the sidebar: who is signed in and how the sync
// is doing; its menu holds the account, the system pages and "Sign out".
defineProps<{ items: { title: string; url: string; icon: LucideIcon }[] }>()

const account = useAccountStore()
const org = useOrgStore()
// Own picture for the sidebar.
const loadMine = async () => {
  await org.loadOrgs().catch(() => {})
  if (org.activeId) await org.loadPresence().catch(() => {})
}
watch(() => account.status.signedIn, (on) => { if (on) void loadMine() }, { immediate: true })
// The dot on the picture is the person's own status, kept current.
let refresh: ReturnType<typeof setInterval> | undefined
let unlisten: (() => void) | undefined
onMounted(async () => {
  refresh = setInterval(() => { if (account.status.signedIn && org.activeId) void org.loadPresence().catch(() => {}) }, 60_000)
  unlisten = await events.presenceChanged.listen((e) => {
    if (e.payload.orgId === org.activeId) void org.loadPresence().catch(() => {})
  })
})
onBeforeUnmount(() => {
  clearInterval(refresh)
  unlisten?.()
})
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
            <span class="relative inline-flex shrink-0">
              <Avatar class="size-8 rounded-full">
                <AvatarImage v-if="org.myAvatar" :src="org.myAvatar" :alt="name ?? ''" class="object-cover" />
                <AvatarFallback class="font-semibold">{{ initial }}</AvatarFallback>
              </Avatar>
              <PresenceDot v-if="account.status.signedIn && org.myId" :user-id="org.myId" class="absolute -right-0.5 -bottom-0.5 ring-2 ring-sidebar" />
            </span>
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
              <span class="relative inline-flex shrink-0">
                <Avatar class="size-8 rounded-full">
                  <AvatarImage v-if="org.myAvatar" :src="org.myAvatar" :alt="name ?? ''" class="object-cover" />
                  <AvatarFallback class="font-semibold">{{ initial }}</AvatarFallback>
                </Avatar>
                <PresenceDot v-if="account.status.signedIn && org.myId" :user-id="org.myId" class="absolute -right-0.5 -bottom-0.5 ring-2 ring-popover" />
              </span>
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
