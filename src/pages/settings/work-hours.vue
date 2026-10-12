<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Plus, Trash2 } from '@lucide/vue'
import { commands, type BalanceAdjustment, type ScheduleModel, type WorkSchedule } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import DateInput from '@/components/time/DateInput.vue'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import CountryPicker from '@/components/time/CountryPicker.vue'
import DurationInput from '@/components/time/DurationInput.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import SettingsNav from '@/components/time/SettingsNav.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { MINUTE, addDays, parseDate, toDateInput } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'
import { useWorkStore } from '@/stores/work'

definePage({ meta: { titleKey: 'nav.settings' } })

const { t } = useI18n()
const work = useWorkStore()
const settings = useSettingsStore()
const format = useFormat()

async function run<T>(promise: Promise<T>, success?: string): Promise<T | undefined> {
  try {
    const result = await promise
    if (success) toast.success(success)
    return result
  } catch (err) {
    toast.error(errorMessage(err))
    return undefined
  }
}

// ----- Country -----
const profileId = ref('AT')
const region = ref<string | null>(null)
watch(
  () => work.jurisdiction,
  (j) => {
    profileId.value = j?.profileId ?? 'AT'
    region.value = j?.region ?? null
  },
  { immediate: true },
)
const countryDirty = computed(
  () => profileId.value !== (work.jurisdiction?.profileId ?? '') || region.value !== (work.jurisdiction?.region ?? null),
)
const saveCountry = () => run(call(commands.jurisdictionSet(profileId.value, region.value)), t('common.saved'))

const hours = (minutes: number | null) => (minutes === null ? t('work.none') : `${settings.formatDuration(minutes * MINUTE)} h`)

// ----- Schedules -----
// Monday 1 June 2026, to label weekdays in the UI language.
const weekdayLabels = computed(() => Array.from({ length: 7 }, (_, i) => format.weekday(addDays(parseDate('2026-06-01'), i))))
const models: ScheduleModel[] = ['flextime', 'fixed', 'trust']

interface ScheduleForm {
  id: string
  validFrom: string
  model: ScheduleModel
  /** Target minutes per day, Monday first; `null` = empty field. */
  minutes: (number | null)[]
}

const forms = ref<ScheduleForm[]>([])
watch(
  () => work.schedules,
  (list) => {
    forms.value = [...list]
      .sort((a, b) => b.validFrom.localeCompare(a.validFrom))
      .map((s: WorkSchedule) => ({
        id: s.id,
        validFrom: s.validFrom,
        model: s.model,
        minutes: s.days.map((d) => d.targetMinutes),
      }))
  },
  { immediate: true },
)

const weekTotal = (form: ScheduleForm) =>
  form.minutes.reduce<number>((sum, m) => sum + (m ?? 0), 0) * MINUTE

function toInput(form: ScheduleForm) {
  const minutes = form.minutes
  if (minutes.some((m) => m !== null && m < 0)) return null
  return {
    validFrom: form.validFrom,
    model: form.model,
    days: minutes.map((m) => ({ targetMinutes: m ?? 0, coreStart: null, coreEnd: null })),
  }
}

async function saveSchedule(form: ScheduleForm) {
  const input = toInput(form)
  if (!input) {
    toast.error(t('entry.invalidTime'))
    return
  }
  await run(call(commands.scheduleSave(form.id, input)), t('common.saved'))
}

const newFrom = ref(toDateInput(addDays(Date.now(), 1)))
async function addSchedule() {
  const latest = forms.value[0]
  const base = latest ? toInput(latest) : null
  const days = base?.days ?? Array.from({ length: 7 }, (_, i) => ({ targetMinutes: i < 5 ? 480 : 0, coreStart: null, coreEnd: null }))
  await run(call(commands.scheduleSave(null, { validFrom: newFrom.value, model: latest?.model ?? 'flextime', days })))
}

const deletingSchedule = ref<string | null>(null)
const confirmScheduleOpen = ref(false)
function askDeleteSchedule(id: string) {
  deletingSchedule.value = id
  confirmScheduleOpen.value = true
}
const deleteSchedule = () => deletingSchedule.value && run(call(commands.scheduleDelete(deletingSchedule.value)))

// ----- Vacation & breaks -----
const vacationDays = ref('')
const deductBreaks = ref(true)
watch(
  () => work.settings,
  (s) => {
    vacationDays.value = s?.vacationDaysPerYear == null ? '' : String(s.vacationDaysPerYear)
    deductBreaks.value = s?.deductBreaks ?? true
  },
  { immediate: true },
)
async function saveWorkSettings() {
  if (!work.settings) return
  const days = vacationDays.value.trim() === '' ? null : Number(vacationDays.value.replace(',', '.'))
  if (days !== null && Number.isNaN(days)) {
    toast.error(t('work.vacationDays'))
    return
  }
  await run(
    call(commands.workSettingsSet({ ...work.settings, vacationDaysPerYear: days, deductBreaks: deductBreaks.value })),
    t('common.saved'),
  )
}

// ----- Balance corrections -----
const adjustments = ref<BalanceAdjustment[]>([])
const loadAdjustments = async () => (adjustments.value = (await run(call(commands.adjustmentList()))) ?? [])
watch(() => work.balance, loadAdjustments, { immediate: true })

const adjDate = ref(toDateInput(Date.now()))
const adjMinutes = ref<number | null>(null)
const adjReason = ref('')
async function addAdjustment() {
  const minutes = adjMinutes.value
  if (minutes === null || minutes === 0) {
    toast.error(t('work.hoursSigned'))
    return
  }
  const ok = await run(call(commands.adjustmentCreate({ date: adjDate.value, minutes, reason: adjReason.value })))
  if (ok) {
    adjMinutes.value = null
    adjReason.value = ''
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.settings')" />
  <SettingsNav />

  <div class="grid max-w-4xl gap-6">
    <Card>
      <CardHeader>
        <CardTitle>{{ $t('work.country') }}</CardTitle>
        <CardDescription>{{ $t('work.countryHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <CountryPicker v-model:profile-id="profileId" v-model:region="region" />
        <div class="flex justify-end">
          <Button :disabled="!countryDirty" @click="saveCountry">{{ $t('common.save') }}</Button>
        </div>
        <dl v-if="work.summary" class="grid gap-x-6 gap-y-2 rounded-lg bg-muted/50 p-4 text-sm sm:grid-cols-2">
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.maxDaily') }}</dt>
            <dd>{{ hours(work.summary.maxDailyMinutes) }}</dd>
          </div>
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.maxWeekly') }}</dt>
            <dd>{{ hours(work.summary.maxWeeklyMinutes) }}</dd>
          </div>
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.minRest') }}</dt>
            <dd>{{ hours(work.summary.minDailyRestMinutes) }}</dd>
          </div>
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.statutoryVacation') }}</dt>
            <dd>{{ $t('work.days', { n: work.summary.vacationDays }) }}</dd>
          </div>
          <div class="flex justify-between gap-2 sm:col-span-2">
            <dt class="text-muted-foreground">{{ $t('work.breaks') }}</dt>
            <dd class="text-right">
              <span v-for="([after, brk], i) in work.summary.breaks" :key="after">
                {{ i ? ' · ' : '' }}{{ $t('work.breakRule', { break: brk, after: format.number(after / 60, 1) }) }}
              </span>
              <span v-if="!work.summary.breaks.length">{{ $t('work.none') }}</span>
            </dd>
          </div>
        </dl>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ $t('work.schedules') }}</CardTitle>
        <CardDescription>{{ $t('work.schedulesHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div v-for="form in forms" :key="form.id" class="grid gap-3 rounded-lg border p-4">
          <div class="flex flex-wrap items-end gap-3">
            <div class="grid gap-2">
              <Label>{{ $t('work.validFrom') }}</Label>
              <DateInput v-model="form.validFrom" class="w-40" />
            </div>
            <div class="grid gap-2">
              <Label>{{ $t('work.model') }}</Label>
              <Select v-model="form.model">
                <SelectTrigger class="w-56"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem v-for="m in models" :key="m" :value="m">{{ $t(`work.models.${m}`) }}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="ml-auto flex gap-2">
              <Button size="icon" variant="ghost" :title="$t('common.delete')" @click="askDeleteSchedule(form.id)"><Trash2 /></Button>
              <Button @click="saveSchedule(form)">{{ $t('common.save') }}</Button>
            </div>
          </div>
          <div class="grid grid-cols-8 gap-2">
            <div v-for="(label, i) in weekdayLabels" :key="i" class="grid gap-1">
              <Label class="text-xs text-muted-foreground">{{ label }}</Label>
              <DurationInput v-model="form.minutes[i]" class="px-2 text-center tabular-nums" />
            </div>
            <div class="grid gap-1">
              <Label class="text-xs text-muted-foreground">{{ $t('work.weekTotal') }}</Label>
              <div class="flex h-9 items-center justify-center text-sm font-medium tabular-nums">
                {{ settings.formatDuration(weekTotal(form)) }}
              </div>
            </div>
          </div>
        </div>
        <div class="flex flex-wrap items-end gap-2">
          <div class="grid gap-2">
            <Label>{{ $t('work.addSchedule') }}</Label>
            <DateInput v-model="newFrom" class="w-40" />
          </div>
          <Button variant="outline" @click="addSchedule"><Plus /> {{ $t('common.create') }}</Button>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ $t('absences.title') }} &amp; {{ $t('work.breaks') }}</CardTitle>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <div>
            <Label for="vacation-days">{{ $t('work.vacationDays') }}</Label>
            <p class="text-xs text-muted-foreground">{{ $t('work.vacationDaysHint', { n: work.summary?.vacationDays ?? 0 }) }}</p>
          </div>
          <Input id="vacation-days" v-model="vacationDays" inputmode="decimal" :placeholder="String(work.summary?.vacationDays ?? '')" />
        </div>
        <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <div>
            <Label>{{ $t('work.deductBreaks') }}</Label>
            <p class="text-xs text-muted-foreground">{{ $t('work.deductBreaksHint') }}</p>
          </div>
          <Switch v-model="deductBreaks" />
        </div>
        <div class="flex justify-end">
          <Button @click="saveWorkSettings">{{ $t('common.save') }}</Button>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ $t('work.adjustments') }}</CardTitle>
        <CardDescription>{{ $t('work.adjustmentsHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <form class="flex flex-wrap items-end gap-2" @submit.prevent="addAdjustment">
          <div class="grid gap-2">
            <Label>{{ $t('entry.date') }}</Label>
            <DateInput v-model="adjDate" class="w-40" required />
          </div>
          <div class="grid gap-2">
            <Label>{{ $t('work.hoursSigned') }}</Label>
            <DurationInput v-model="adjMinutes" signed class="w-28" required />
          </div>
          <div class="grid flex-1 gap-2">
            <Label>{{ $t('work.reason') }}</Label>
            <Input v-model="adjReason" required />
          </div>
          <Button type="submit" variant="outline"><Plus /> {{ $t('work.addAdjustment') }}</Button>
        </form>
        <p v-if="adjustments.length === 0" class="text-sm text-muted-foreground">{{ $t('work.noAdjustments') }}</p>
        <ul v-else class="divide-y rounded-lg border text-sm">
          <li v-for="a in adjustments" :key="a.id" class="flex items-center gap-3 px-3 py-2">
            <span class="w-28 text-muted-foreground">{{ format.longDate(parseDate(a.date)) }}</span>
            <span class="w-20 font-medium tabular-nums">{{ settings.formatSigned(a.minutes * MINUTE) }}</span>
            <span class="flex-1 truncate">{{ a.reason }}</span>
            <Button size="icon" variant="ghost" class="size-7" @click="run(call(commands.adjustmentDelete(a.id)))"><Trash2 /></Button>
          </li>
        </ul>
      </CardContent>
    </Card>
  </div>

  <ConfirmDialog v-model:open="confirmScheduleOpen" :title="$t('work.deleteSchedule')" @confirm="deleteSchedule" />
</template>
