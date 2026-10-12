<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { FileJson, FolderOpen, HardDriveDownload } from '@lucide/vue'
import { commands, type BackupInfo } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import PageHeader from '@/components/time/PageHeader.vue'
import SettingsNav from '@/components/time/SettingsNav.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { parseDate } from '@/lib/time'

definePage({ meta: { titleKey: 'nav.settings' } })

const { t } = useI18n()
const format = useFormat()
const backups = ref<BackupInfo | null>(null)
const busy = ref(false)

onMounted(async () => {
  backups.value = await call(commands.backupInfo()).catch(() => null)
})

async function run<T>(action: () => Promise<T>) {
  busy.value = true
  try {
    return await action()
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    busy.value = false
  }
}

async function exportJson() {
  const path = await run(() => call(commands.dataExportJson()))
  if (path) toast.success(t('reports.exported', { path }))
}

async function backupNow() {
  const info = await run(() => call(commands.backupNow()))
  if (info) {
    backups.value = info
    toast.success(t('settings.data.backupDone'))
  }
}

/** `time-2026-10-10.db` → localized date. */
function backupDate(file: string) {
  const date = file.match(/\d{4}-\d{2}-\d{2}/)?.[0]
  return date ? format.longDate(parseDate(date)) : file
}
</script>

<template>
  <PageHeader :title="$t('nav.settings')" />
  <SettingsNav />

  <div class="grid max-w-3xl gap-6">
    <Card>
      <CardHeader>
        <CardTitle>{{ $t('settings.data.exportTitle') }}</CardTitle>
        <CardDescription>{{ $t('settings.data.exportHint') }}</CardDescription>
      </CardHeader>
      <CardContent>
        <Button variant="outline" :disabled="busy" @click="exportJson"><FileJson /> {{ $t('settings.data.exportJson') }}</Button>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ $t('settings.data.backupsTitle') }}</CardTitle>
        <CardDescription>{{ $t('settings.data.backupsHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="flex flex-wrap gap-2">
          <Button variant="outline" :disabled="busy" @click="backupNow"><HardDriveDownload /> {{ $t('settings.data.backupNow') }}</Button>
          <Button variant="ghost" @click="run(() => call(commands.backupOpenDir()))"><FolderOpen /> {{ $t('settings.data.openFolder') }}</Button>
        </div>
        <p v-if="backups" class="truncate font-mono text-xs text-muted-foreground" :title="backups.dir">{{ backups.dir }}</p>
        <ul v-if="backups?.files.length" class="divide-y rounded-lg border text-sm">
          <li v-for="file in backups.files" :key="file" class="flex items-center justify-between gap-3 px-3 py-1.5">
            <span>{{ backupDate(file) }}</span>
            <span class="font-mono text-xs text-muted-foreground">{{ file }}</span>
          </li>
        </ul>
        <p v-else class="text-sm text-muted-foreground">{{ $t('settings.data.noBackups') }}</p>
      </CardContent>
    </Card>
  </div>
</template>
