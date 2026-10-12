<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import type { AppSettings, DateFormat, DurationFormat, NumberFormat, TimeFormat, WeekStart } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import ThemeSwitch from '@/components/ThemeSwitch.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import SettingsNav from '@/components/time/SettingsNav.vue'
import TimezonePicker from '@/components/time/TimezonePicker.vue'
import { errorMessage } from '@/lib/ipc'
import { useSettingsStore } from '@/stores/settings'

definePage({ meta: { titleKey: 'nav.settings' } })

const { t } = useI18n()
const store = useSettingsStore()

const form = ref<AppSettings | null>(null)
watch(
  () => store.settings,
  (s) => (form.value = s ? { ...s } : null),
  { immediate: true },
)
const dirty = computed(() => JSON.stringify(form.value) !== JSON.stringify(store.settings))

// Select items cannot have an empty value.
const SYSTEM = 'system'
const locale = computed({
  get: () => form.value?.locale ?? SYSTEM,
  set: (v: string) => form.value && (form.value.locale = v === SYSTEM ? null : v),
})
const formats: DurationFormat[] = ['hm', 'decimal']

// Regional formats; the examples show what each option looks like.
const dateFormats: { value: DateFormat; example: string }[] = [
  { value: 'auto', example: '' },
  { value: 'dmy', example: '31.12.2026' },
  { value: 'mdy', example: '12/31/2026' },
  { value: 'ymd', example: '2026-12-31' },
]
const timeFormats: { value: TimeFormat; example: string }[] = [
  { value: 'auto', example: '' },
  { value: 'h24', example: '14:30' },
  { value: 'h12', example: '2:30 PM' },
]
const numberFormats: { value: NumberFormat; example: string }[] = [
  { value: 'auto', example: '' },
  { value: 'point', example: '7.5' },
  { value: 'comma', example: '7,5' },
]
const weekStarts: WeekStart[] = ['auto', 'monday', 'sunday', 'saturday']

/** Label of an "automatic" option: what the system locale uses. */
const examples: Record<string, string> = {
  dmy: '31.12.2026',
  mdy: '12/31/2026',
  ymd: '2026-12-31',
  h24: '14:30',
  h12: '2:30 PM',
  point: '7.5',
  comma: '7,5',
}
const system = computed(() => store.info?.systemFormats)
const autoLabel = (example: string | undefined) => t('settings.region.auto', { example: example ?? '' })

/** `Etc/GMT-1` → `UTC+1` (Etc zones have inverted signs). */
function zoneLabel(zone: string) {
  const m = /^Etc\/GMT([+-])(\d+)$/.exec(zone)
  if (m) return `UTC${m[1] === '-' ? '+' : '−'}${m[2]}`
  return zone === 'Etc/GMT' ? 'UTC' : zone
}

const shortcuts = [
  { key: 'shortcutToggleWindow', label: 'settings.shortcutToggle' },
  { key: 'shortcutQuickSwitch', label: 'settings.shortcutQuickSwitch' },
  { key: 'shortcutPauseResume', label: 'settings.shortcutPauseResume' },
] as const

const saving = ref(false)
async function save() {
  if (!form.value) return
  saving.value = true
  try {
    await store.save(form.value)
    toast.success(t('common.saved'))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.settings')">
    <Button :disabled="!dirty || saving" @click="save">{{ $t('common.save') }}</Button>
  </PageHeader>
  <SettingsNav />

  <div v-if="form" class="grid max-w-3xl gap-6">
    <Card>
      <CardHeader>
        <CardTitle>{{ $t('settings.general') }}</CardTitle>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.language') }}</Label>
          <Select v-model="locale">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem :value="SYSTEM">{{ $t('settings.languageSystem') }}</SelectItem>
              <SelectItem value="en">English</SelectItem>
              <SelectItem value="de">Deutsch</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.theme') }}</Label>
          <ThemeSwitch />
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.durationFormat') }}</Label>
          <Select v-model="form.durationFormat">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="f in formats" :key="f" :value="f">{{ $t(`settings.formats.${f}`) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <div>
            <Label>{{ $t('settings.autostart') }}</Label>
            <p class="text-xs text-muted-foreground">{{ $t('settings.autostartHint') }}</p>
          </div>
          <Switch v-model="form.autostart" />
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ $t('settings.region.title') }}</CardTitle>
        <CardDescription>
          {{ $t('settings.region.hint') }}
          <template v-if="store.info"> {{ $t('settings.region.systemLocale', { locale: store.info.systemLocale }) }}</template>
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.region.timezone') }}</Label>
          <TimezonePicker
            :model-value="form.timezone ?? null"
            :system-zone="store.info?.systemTz ?? 'UTC'"
            @update:model-value="form.timezone = $event"
          />
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <div>
            <Label>{{ $t('settings.region.daylightSaving') }}</Label>
            <p class="text-xs text-muted-foreground">
              {{ form.daylightSaving === false ? $t('settings.region.noDstHint', { zone: zoneLabel(store.info?.defaultTz ?? '') }) : $t('settings.region.dstHint') }}
            </p>
          </div>
          <Switch :model-value="form.daylightSaving ?? true" @update:model-value="form.daylightSaving = $event" />
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.region.dateFormat') }}</Label>
          <Select v-model="form.dateFormat">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="f in dateFormats" :key="f.value" :value="f.value">{{ f.example || autoLabel(examples[system?.date ?? '']) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.region.timeFormat') }}</Label>
          <Select v-model="form.timeFormat">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="f in timeFormats" :key="f.value" :value="f.value">{{ f.example || autoLabel(examples[system?.time ?? '']) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.region.numberFormat') }}</Label>
          <Select v-model="form.numberFormat">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="f in numberFormats" :key="f.value" :value="f.value">{{ f.example || autoLabel(examples[system?.number ?? '']) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('settings.region.weekStart') }}</Label>
          <Select v-model="form.weekStart">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="d in weekStarts" :key="d" :value="d">
                {{ d === 'auto' ? autoLabel(system ? $t(`settings.region.weekdays.${system.weekStart}`) : '') : $t(`settings.region.weekdays.${d}`) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <p class="text-xs text-muted-foreground">{{ $t('settings.region.storageHint') }}</p>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ $t('settings.shortcuts') }}</CardTitle>
        <CardDescription>{{ $t('settings.shortcutHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div v-for="s in shortcuts" :key="s.key" class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label :for="s.key">{{ $t(s.label) }}</Label>
          <Input :id="s.key" v-model="form[s.key]" class="font-mono" spellcheck="false" />
        </div>
      </CardContent>
    </Card>

    <Card v-if="store.info">
      <CardHeader>
        <CardTitle>{{ $t('settings.about') }}</CardTitle>
      </CardHeader>
      <CardContent>
        <dl class="grid gap-2 text-sm sm:grid-cols-[12rem_1fr]">
          <dt class="text-muted-foreground">{{ $t('settings.version') }}</dt>
          <dd>{{ store.info.version }}</dd>
          <dt class="text-muted-foreground">{{ $t('settings.timezone') }}</dt>
          <dd>{{ store.info.defaultTz }}</dd>
          <dt class="text-muted-foreground">{{ $t('settings.dataDir') }}</dt>
          <dd class="font-mono text-xs break-all">{{ store.info.dataDir }}</dd>
          <dt class="text-muted-foreground">{{ $t('settings.deviceId') }}</dt>
          <dd class="font-mono text-xs break-all">{{ store.info.deviceId }}</dd>
          <dt class="text-muted-foreground">{{ $t('settings.pendingChanges') }}</dt>
          <dd>{{ store.info.pendingChanges }}</dd>
        </dl>
      </CardContent>
    </Card>
  </div>
</template>
