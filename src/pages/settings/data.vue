<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { FileInput, FileJson, FolderOpen, HardDriveDownload } from '@lucide/vue'
import { commands, type BackupInfo, type DateOrder, type ImportPreview } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
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

// ------------------------------------------------------------------ import
const file = ref<{ name: string; text: string } | null>(null)
const order = ref<DateOrder>('ymd')
const preview = ref<ImportPreview | null>(null)
const ORDERS: DateOrder[] = ['ymd', 'dmy', 'mdy']

/** The order the first date of the file suggests; slashes could be either, so the US order is the guess. */
function guessOrder(text: string): DateOrder {
  const row = text.split('\n')[1] ?? ''
  if (/\b\d{4}-\d{2}-\d{2}\b/.test(row)) return 'ymd'
  if (/\b\d{1,2}\.\d{1,2}\.\d{2,4}\b/.test(row)) return 'dmy'
  return 'mdy'
}
async function pickImport() {
  const picked = await run(() => call(commands.importPick()))
  if (!picked) return
  file.value = picked
  order.value = guessOrder(picked.text)
  await refreshPreview()
}
async function refreshPreview() {
  if (!file.value) return
  preview.value = null
  const text = file.value.text
  preview.value = (await run(() => call(commands.importPreview(text, order.value)))) ?? null
}
async function runImport() {
  if (!file.value) return
  const text = file.value.text
  const report = await run(() => call(commands.importRun(text, order.value)))
  if (!report) return
  toast.success(t('settings.data.imported', { n: report.createdEntries, skipped: report.duplicates }))
  file.value = null
  preview.value = null
}

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
        <CardTitle>{{ $t('settings.data.importTitle') }}</CardTitle>
        <CardDescription>{{ $t('settings.data.importHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="flex flex-wrap items-center gap-3">
          <Button variant="outline" :disabled="busy" @click="pickImport"><FileInput /> {{ $t('settings.data.importChoose') }}</Button>
          <span v-if="file" class="truncate font-mono text-xs text-muted-foreground">{{ file.name }}</span>
        </div>
        <div v-if="file" class="grid max-w-xs gap-2">
          <Label>{{ $t('settings.data.importOrder') }}</Label>
          <Select v-model="order" @update:model-value="refreshPreview">
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="o in ORDERS" :key="o" :value="o">{{ $t(`settings.data.orders.${o}`) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div v-if="preview" class="grid gap-2 text-sm">
          <p>{{ $t('settings.data.importSummary', { n: preview.newEntries, dup: preview.duplicates }) }}</p>
          <p v-if="preview.firstAt && preview.lastAt" class="text-muted-foreground">{{ format.longDate(preview.firstAt) }} – {{ format.longDate(preview.lastAt) }}</p>
          <p v-if="preview.newProjects.length" class="text-muted-foreground">{{ $t('settings.data.importProjects', { list: preview.newProjects.slice(0, 8).join(', ') + (preview.newProjects.length > 8 ? ' …' : '') }) }}</p>
          <p v-if="preview.newTags.length" class="text-muted-foreground">{{ $t('settings.data.importTags', { list: preview.newTags.slice(0, 8).join(', ') + (preview.newTags.length > 8 ? ' …' : '') }) }}</p>
          <div v-if="preview.skipped.length" class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
            <p class="font-medium">{{ $t('settings.data.importSkipped', { n: preview.skipped.length }) }}</p>
            <p v-for="s in preview.skipped.slice(0, 5)" :key="s.line" class="text-muted-foreground">{{ $t('settings.data.importLine', { line: s.line, reason: s.reason }) }}</p>
          </div>
          <div><Button :disabled="busy || preview.newEntries === 0" @click="runImport">{{ $t('settings.data.importRun', { n: preview.newEntries }) }}</Button></div>
        </div>
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
