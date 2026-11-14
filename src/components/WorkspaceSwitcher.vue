<script setup lang="ts">
import { toast } from 'vue-sonner'
import { Building2, Check, ChevronsUpDown, User } from '@lucide/vue'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar'
import { errorMessage } from '@/lib/ipc'
import { useWorkspaceStore } from '@/stores/workspace'

const workspaces = useWorkspaceStore()

async function pick(id: string) {
  try {
    await workspaces.switchTo(id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <SidebarMenuItem v-if="workspaces.switchable">
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <SidebarMenuButton :tooltip="workspaces.active?.name ?? ''" class="justify-between">
          <span class="flex min-w-0 items-center gap-2">
            <Building2 v-if="workspaces.inOrg" class="size-4 shrink-0" />
            <User v-else class="size-4 shrink-0" />
            <span class="truncate">{{ workspaces.inOrg ? workspaces.active?.name : $t('app.personal') }}</span>
          </span>
          <ChevronsUpDown class="size-4 shrink-0 opacity-60" />
        </SidebarMenuButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" class="min-w-56">
        <DropdownMenuItem v-for="w in workspaces.all" :key="w.id" class="gap-2" @click="pick(w.id)">
          <User v-if="w.kind === 'personal'" class="size-4" />
          <Building2 v-else class="size-4" />
          <span class="flex-1 truncate">{{ w.kind === 'personal' ? $t('app.personal') : w.name }}</span>
          <Check v-if="w.active" class="size-4" />
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </SidebarMenuItem>
</template>
