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
import DurationInput from '@/components/time/DurationInput.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { MINUTE, addDays, parseDate, toDateInput } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'
import { useWorkStore } from '@/stores/work'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'
import TableToolbar from '@/components/time/TableToolbar.vue'
import { useTableSearch } from '@/composables/useTableSearch'


/**
 * A person's schedule, vacation entitlement, break rules and overtime corrections. In the personal workspace the person
 * sets them; in an organization they are the organization's records, set for a member (`managed`) by someone who may
 * manage members, and read-only for the member (`readOnly`). The server enforces the same rule.
 */
const props = defineProps<{ userId?: string | null; managed?: boolean; readOnly?: boolean }>()

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

const schedules = ref<WorkSchedule[]>([])
const workSettings = ref(work.settings)
async function loadManaged() {
  if (!props.managed || !props.userId) {
    schedules.value = work.schedules
    workSettings.value = work.settings
    return
  }
  schedules.value = (await run(call(commands.managedScheduleList(props.userId)))) ?? []
  workSettings.value = (await run(call(commands.managedSettingsGet(props.userId)))) ?? work.settings
  adjustments.value = (await run(call(commands.managedAdjustmentList(props.userId)))) ?? []
}

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
  () => schedules.value,
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
  await run(props.managed ? call(commands.managedScheduleSave((props.userId ?? ''), form.id, input)) : call(commands.scheduleSave(form.id, input)), t('common.saved'))
  await loadManaged()
}

const newFrom = ref(toDateInput(addDays(Date.now(), 1)))
async function addSchedule() {
  const latest = forms.value[0]
  const base = latest ? toInput(latest) : null
  const days = base?.days ?? Array.from({ length: 7 }, (_, i) => ({ targetMinutes: i < 5 ? 480 : 0, coreStart: null, coreEnd: null }))
  const created = { validFrom: newFrom.value, model: latest?.model ?? 'flextime', days }
  await run(props.managed ? call(commands.managedScheduleSave((props.userId ?? ''), null, created)) : call(commands.scheduleSave(null, created)))
  await loadManaged()
}

const deletingSchedule = ref<string | null>(null)
const confirmScheduleOpen = ref(false)
function askDeleteSchedule(id: string) {
  deletingSchedule.value = id
  confirmScheduleOpen.value = true
}
const deleteSchedule = async () => {
  if (!deletingSchedule.value) return
  await run(props.managed ? call(commands.managedScheduleDelete((props.userId ?? ''), deletingSchedule.value)) : call(commands.scheduleDelete(deletingSchedule.value)))
  await loadManaged()
}

// ----- Vacation & breaks -----
const vacationDays = ref('')
const deductBreaks = ref(true)
watch(
  () => workSettings.value,
  (s) => {
    vacationDays.value = s?.vacationDaysPerYear == null ? '' : String(s.vacationDaysPerYear)
    deductBreaks.value = s?.deductBreaks ?? true
  },
  { immediate: true },
)
async function saveWorkSettings() {
  if (!workSettings.value) return
  const days = vacationDays.value.trim() === '' ? null : Number(vacationDays.value.replace(',', '.'))
  if (days !== null && Number.isNaN(days)) {
    toast.error(t('work.vacationDays'))
    return
  }
  const next = { ...workSettings.value, vacationDaysPerYear: days, deductBreaks: deductBreaks.value }
  await run(props.managed ? call(commands.managedSettingsSet((props.userId ?? ''), next)) : call(commands.workSettingsSet(next)), t('common.saved'))
  await loadManaged()
}

// ----- Balance corrections -----
const adjustments = ref<BalanceAdjustment[]>([])
const loadAdjustments = async () => {
  if (props.managed) return loadManaged()
  adjustments.value = (await run(call(commands.adjustmentList()))) ?? []
}
// The records follow the workspace, the person picked and whatever the app syncs in.
watch([() => work.balance, () => work.schedules, () => work.settings, () => props.userId, () => props.managed], async () => {
  await loadManaged()
  if (!props.managed) await loadAdjustments()
}, { immediate: true })

const adjDate = ref(toDateInput(Date.now()))
const adjMinutes = ref<number | null>(null)
const adjReason = ref('')
async function addAdjustment() {
  const minutes = adjMinutes.value
  if (minutes === null || minutes === 0) {
    toast.error(t('work.hoursSigned'))
    return
  }
  const input = { date: adjDate.value, minutes, reason: adjReason.value }
  const ok = await run(props.managed ? call(commands.managedAdjustmentCreate((props.userId ?? ''), input)) : call(commands.adjustmentCreate(input)))
  await loadManaged()
  if (ok) {
    adjMinutes.value = null
    adjReason.value = ''
  }
}

async function deleteAdjustment(id: string) {
  await run(props.managed ? call(commands.managedAdjustmentDelete((props.userId ?? ''), id)) : call(commands.adjustmentDelete(id)))
  await loadManaged()
}

const { search: adjSearch, filtered: shownAdjustments, searching: adjSearching } = useTableSearch(adjustments, (a) => [a.date, a.reason, a.minutes])
const { sort, sorted: sortedAdjustments } = useSort(shownAdjustments, { date: (a) => a.date, minutes: (a) => a.minutes, reason: (a) => a.reason })
</script>

<template>
  <div class="grid gap-6">
    <Card :inert="props.readOnly" :class="{ 'opacity-60': props.readOnly }">
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

    <Card :inert="props.readOnly" :class="{ 'opacity-60': props.readOnly }">
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

    <Card :inert="props.readOnly" :class="{ 'opacity-60': props.readOnly }">
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
        <template v-else>
        <TableToolbar v-model="adjSearch" class="!mb-0" :active="adjSearching" @clear="adjSearch = ''" />
        <p v-if="sortedAdjustments.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
        <div v-else class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <SortHead :sort="sort" column="date" class="px-3 py-2 font-medium">{{ $t('work.adjustmentDate') }}</SortHead>
                <SortHead :sort="sort" column="minutes" class="px-3 py-2 text-right font-medium">{{ $t('work.adjustmentMinutes') }}</SortHead>
                <SortHead :sort="sort" column="reason" class="px-3 py-2 font-medium">{{ $t('work.reason') }}</SortHead>
                <th class="w-12 px-1 py-2"><span class="sr-only">{{ $t('common.delete') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="a in sortedAdjustments" :key="a.id" class="group border-b last:border-b-0 hover:bg-muted/30">
                <td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">{{ format.longDate(parseDate(a.date)) }}</td>
                <td class="whitespace-nowrap px-3 py-1.5 text-right font-medium tabular-nums">{{ settings.formatSigned(a.minutes * MINUTE) }}</td>
                <td class="max-w-0 w-full truncate px-3 py-1.5">{{ a.reason }}</td>
                <td class="px-1 py-0.5">
                  <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="deleteAdjustment(a.id)"><Trash2 /></Button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        </template>
      </CardContent>
    </Card>
  </div>
  <ConfirmDialog v-model:open="confirmScheduleOpen" :title="$t('work.deleteSchedule')" @confirm="deleteSchedule" />
</template>
