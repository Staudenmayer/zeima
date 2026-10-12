<template>
  <SidebarProvider>
    <AppSidebar />
    <SidebarInset :class="route.meta.fill ? 'h-svh' : ''">
      <header class="flex h-14 shrink-0 items-center gap-2 border-b px-4">
        <SidebarTrigger class="-ml-1" />
        <Separator orientation="vertical" class="mr-2 data-[orientation=vertical]:h-4" />
        <span class="truncate font-medium">{{ route.meta.titleKey ? $t(route.meta.titleKey) : '' }}</span>
        <div class="ml-auto flex min-w-0 items-center gap-2">
          <TimerPill />
        </div>
      </header>
      <main :class="route.meta.fill ? 'flex min-h-0 flex-1 flex-col overflow-y-auto p-6' : 'p-6'">
        <slot />
      </main>
      <OnboardingDialog />
    </SidebarInset>
  </SidebarProvider>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'
import AppSidebar from '@/components/AppSidebar.vue'
import OnboardingDialog from '@/components/time/OnboardingDialog.vue'
import TimerPill from '@/components/time/TimerPill.vue'
import { Separator } from '@/components/ui/separator'
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'

const route = useRoute()
</script>
