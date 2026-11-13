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
      <SetupWizard />
    </SidebarInset>
  </SidebarProvider>
</template>

<script setup lang="ts">
import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
import { toast } from 'vue-sonner'
import AppSidebar from '@/components/AppSidebar.vue'
import SetupWizard from '@/components/time/SetupWizard.vue'
import TimerPill from '@/components/time/TimerPill.vue'
import { Separator } from '@/components/ui/separator'
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import { useUpdatesStore } from '@/stores/updates'

const route = useRoute()
const { t } = useI18n()
const updates = useUpdatesStore()

// A quiet look for a newer version shortly after the app starts. Offline, or
// in a development build, it finds nothing and says nothing.
onMounted(() => {
  setTimeout(async () => {
    try {
      const found = await updates.check()
      if (found) {
        toast(t('settings.updateAvailable', { version: found.version }), {
          duration: 15_000,
          action: { label: t('settings.updateInstallShort'), onClick: () => void updates.install() },
        })
      }
    } catch {
      // not worth a message
    }
  }, 15_000)
})
</script>
