<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Check, Plus, X } from '@lucide/vue'
import { commands, type AwayAction, type DateFormat, type DurationFormat, type TimeFormat, type WeekStart } from '@/bindings'
import ThemeSwitch from '@/components/ThemeSwitch.vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { PALETTE, nextColor } from '@/lib/colors'
import { call, errorMessage } from '@/lib/ipc'
import { toDateInput } from '@/lib/time'
import { useActivitiesStore } from '@/stores/activities'
import { useCostCentersStore, costCenterLabel } from '@/stores/costCenters'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useSetupStore } from '@/stores/setup'
import { useWorkStore } from '@/stores/work'
import CountryPicker from './CountryPicker.vue'

// Setup of the whole workspace. Opens on the first start (until the country
// is saved) and again from Settings. Every choice is kept here and applied
// together at the end; the country goes last, because saving it is what ends
// the first-start setup: quitting half way starts it again.
const { t } = useI18n()
const work = useWorkStore()
const settings = useSettingsStore()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const costCenters = useCostCentersStore()
const setup = useSetupStore()

const STEPS = ['you', 'workWeek', 'activities', 'projects', 'costCenters', 'absences', 'autoPause', 'review'] as const
const LAST = STEPS.length - 1

const open = ref(false)
const step = ref(0)
const saving = ref(false)
const key = computed(() => STEPS[step.value])

// 1. You
const profileId = ref('AT')
const region = ref<string | null>(null)
const prefs = reactive({
  durationFormat: 'hm' as DurationFormat,
  weekStart: 'auto' as WeekStart,
  dateFormat: 'auto' as DateFormat,
  timeFormat: 'auto' as TimeFormat,
  autostart: false,
})
// 2. Work week (Monday first)
const week = reactive({ days: [true, true, true, true, true, false, false], hours: 8, vacation: 30, deductBreaks: true })
// 3. Activities, 4. Projects, 5. Cost centers, 6. Absence types
const picked = ref<string[]>([])
const customActivity = ref('')
const projectForm = reactive({ name: '', alias: '' })
const newProjects = ref<{ name: string; alias: string }[]>([])
const ccForm = reactive({ code: '', name: '' })
const newCostCenters = ref<{ code: string; name: string }[]>([])
const defaultKey = ref<string | null>(null)
const pickedAbsences = ref<string[]>([])
// 7. Auto-pause
const autoPause = reactive({ enabled: true, idleMinutes: 10, idleAction: 'prompt' as AwayAction, lunch: false })

const today = () => toDateInput(Date.now())

function reset() {
  prefs.durationFormat = settings.settings?.durationFormat ?? 'hm'
  prefs.weekStart = settings.settings?.weekStart ?? 'auto'
  prefs.dateFormat = settings.settings?.dateFormat ?? 'auto'
  prefs.timeFormat = settings.settings?.timeFormat ?? 'auto'
  prefs.autostart = settings.settings?.autostart ?? false
  const latest = [...work.schedules].sort((a, b) => a.validFrom.localeCompare(b.validFrom)).pop()
  if (latest) {
    week.days = Array.from({ length: 7 }, (_, i) => (latest.days[i]?.targetMinutes ?? 0) > 0)
    week.hours = (latest.days.find((d) => d.targetMinutes > 0)?.targetMinutes ?? 480) / 60
  }
  week.vacation = work.settings?.vacationDaysPerYear ?? 30
  week.deductBreaks = work.settings?.deductBreaks ?? true
  autoPause.enabled = work.settings?.autoPause?.enabled ?? true
  autoPause.idleMinutes = work.settings?.autoPause?.idleMinutes ?? 10
  autoPause.idleAction = work.settings?.autoPause?.idleAction ?? 'prompt'
  autoPause.lunch = work.settings?.autoPause?.lunchDetection ?? false
  picked.value = []
  newProjects.value = []
  newCostCenters.value = []
  defaultKey.value = null
  pickedAbsences.value = []
}

watch(
  [() => work.needsOnboarding, () => setup.requested],
  async ([first, again]) => {
    if (!first && !again) return
    reset()
    profileId.value = work.jurisdiction?.profileId ?? (await commands.profileSuggest()) ?? 'AT'
    region.value = work.jurisdiction?.region ?? null
    step.value = 0
    open.value = true
  },
  { immediate: true },
)

function close() {
  open.value = false
  setup.requested = false
}

// Activities
const suggestions = computed(() => {
  const taken = new Set(activities.all.map((a) => a.name.toLowerCase()))
  const names = t('activities.suggestions')
    .split(',')
    .map((n) => n.trim())
    .filter((n) => n && !taken.has(n.toLowerCase()))
  return [...new Set([...names, ...picked.value])]
})
const toggle = (list: string[], name: string) => (list.includes(name) ? list.filter((n) => n !== name) : [...list, name])
function addCustomActivity() {
  const name = customActivity.value.trim()
  if (name && !picked.value.some((n) => n.toLowerCase() === name.toLowerCase())) picked.value = [...picked.value, name]
  customActivity.value = ''
}

// Projects
function addProject() {
  const name = projectForm.name.trim()
  if (!name) return
  if (!newProjects.value.some((p) => p.name.toLowerCase() === name.toLowerCase())) {
    newProjects.value = [...newProjects.value, { name, alias: (projectForm.alias.trim() || name.slice(0, 3)).toUpperCase() }]
  }
  projectForm.name = ''
  projectForm.alias = ''
}

// Cost centers
function addCostCenter() {
  const code = ccForm.code.trim()
  const name = ccForm.name.trim()
  if (!code || !name) return
  if (!newCostCenters.value.some((c) => c.code.toLowerCase() === code.toLowerCase())) {
    newCostCenters.value = [...newCostCenters.value, { code, name }]
  }
  ccForm.code = ''
  ccForm.name = ''
}
function removeCostCenter(code: string) {
  newCostCenters.value = newCostCenters.value.filter((c) => c.code !== code)
  if (defaultKey.value === `new:${code}`) defaultKey.value = null
}
const NONE = '__none__'
const defaultOptions = computed(() => [
  ...costCenters.all.filter((c) => c.active).map((c) => ({ key: c.id, label: costCenterLabel(c) })),
  ...newCostCenters.value.map((c) => ({ key: `new:${c.code}`, label: `${c.code} · ${c.name}` })),
])
const defaultValue = computed({
  get: () => defaultKey.value ?? NONE,
  set: (v: string) => (defaultKey.value = v === NONE ? null : v),
})
const defaultLabel = computed(() => defaultOptions.value.find((o) => o.key === defaultKey.value)?.label ?? null)

// Absence types: vacation and sick leave exist already.
const ABSENCE_TYPES = [
  { key: 'parental', reduces: true },
  { key: 'training', reduces: false },
  { key: 'remote', reduces: false },
  { key: 'unpaid', reduces: true },
]
const absenceOptions = computed(() => {
  const taken = new Set(work.absenceTypes.map((a) => work.absenceTypeName(a).toLowerCase()))
  return ABSENCE_TYPES.map((a) => ({ ...a, name: t(`setup.absences.types.${a.key}`) })).filter((a) => !taken.has(a.name.toLowerCase()))
})

const weekStarts: WeekStart[] = ['auto', 'monday', 'sunday', 'saturday']
const dateFormats: { value: DateFormat; label: string }[] = [
  { value: 'dmy', label: '31.12.2026' },
  { value: 'mdy', label: '12/31/2026' },
  { value: 'ymd', label: '2026-12-31' },
]
const timeFormats: { value: TimeFormat; label: string }[] = [
  { value: 'h24', label: '14:30' },
  { value: 'h12', label: '2:30 PM' },
]
const durationFormats: DurationFormat[] = ['hm', 'decimal']
const awayActions: AwayAction[] = ['prompt', 'exclude', 'keep']

const weekdayLabel = (i: number) => new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: 'short' })

const summary = computed(() => {
  const none = t('setup.review.none')
  const days = week.days.map((on, i) => (on ? weekdayLabel(i) : '')).filter(Boolean).join(', ') || none
  return [
    { label: t('setup.review.region'), value: `${profileId.value}${region.value ? ` · ${region.value}` : ''}` },
    { label: t('setup.review.workWeek'), value: t('setup.review.workWeekValue', { hours: week.hours, days, vacation: week.vacation }) },
    { label: t('setup.review.activities'), value: picked.value.join(', ') || none },
    { label: t('setup.review.projects'), value: newProjects.value.map((p) => p.name).join(', ') || none },
    { label: t('setup.review.costCenters'), value: newCostCenters.value.map((c) => c.code).join(', ') || none },
    { label: t('setup.review.defaultCostCenter'), value: defaultLabel.value ?? t('setup.review.notSet') },
    { label: t('setup.review.absenceTypes'), value: pickedAbsences.value.join(', ') || none },
    {
      label: t('setup.review.autoPause'),
      value: autoPause.enabled
        ? t('setup.review.autoPauseOn', { minutes: autoPause.idleMinutes }) + (autoPause.lunch ? `, ${t('setup.review.lunch')}` : '')
        : t('setup.review.off'),
    },
  ]
})

const exists = (list: { name: string }[], name: string) => list.some((x) => x.name.toLowerCase() === name.toLowerCase())

/** Applies everything. The country goes last. */
async function apply() {
  if (settings.settings) {
    await settings.save({
      ...settings.settings,
      durationFormat: prefs.durationFormat,
      weekStart: prefs.weekStart,
      dateFormat: prefs.dateFormat,
      timeFormat: prefs.timeFormat,
      autostart: prefs.autostart,
    })
  }
  if (work.settings) {
    await call(
      commands.workSettingsSet({
        ...work.settings,
        vacationDaysPerYear: week.vacation,
        deductBreaks: week.deductBreaks,
        autoPause: {
          ...work.settings.autoPause,
          enabled: autoPause.enabled,
          idleMinutes: autoPause.idleMinutes,
          idleAction: autoPause.idleAction,
          lunchDetection: autoPause.lunch,
        },
      }),
    )
  }
  const validFrom = `${new Date().getFullYear()}-01-01`
  const minutes = Math.round(week.hours * 60)
  await call(
    commands.scheduleSave(work.schedules.find((s) => s.validFrom === validFrom)?.id ?? null, {
      validFrom,
      model: work.schedules[0]?.model ?? 'flextime',
      days: week.days.map((on) => ({ targetMinutes: on ? minutes : 0, coreStart: null, coreEnd: null })),
    }),
  )
  for (const name of picked.value) {
    if (exists(activities.all, name)) continue
    await activities.create({ name, color: nextColor(activities.all.map((a) => a.color)), archived: false })
    await activities.load()
  }
  // Cost centers first: the default is assigned to the new projects.
  for (const c of newCostCenters.value) {
    if (costCenters.all.some((x) => x.code.toLowerCase() === c.code.toLowerCase())) continue
    await costCenters.save(null, { code: c.code, name: c.name, parentId: null, active: true, validFrom: null, validTo: null })
  }
  await costCenters.load()
  const defaultId = defaultKey.value?.startsWith('new:')
    ? costCenters.all.find((c) => c.code === defaultKey.value!.slice(4))?.id ?? null
    : defaultKey.value
  if (defaultId) {
    await call(commands.costCenterAssignmentSave(null, { costCenterId: defaultId, validFrom: today(), validTo: null }))
  }
  for (const p of newProjects.value) {
    if (exists(projects.all, p.name)) continue
    await projects.create({
      name: p.name,
      alias: p.alias || null,
      color: PALETTE[projects.all.length % PALETTE.length],
      parentId: null,
      client: null,
      budgetMinutes: null,
      budgetPeriod: 'total',
      budgetWarnPct: 80,
      billableDefault: false,
      status: 'active',
      costCenterId: defaultId,
    })
    await projects.load()
  }
  for (const name of pickedAbsences.value) {
    const type = ABSENCE_TYPES.find((a) => t(`setup.absences.types.${a.key}`) === name)
    await call(
      commands.absenceTypeSave(null, {
        name,
        color: PALETTE[(work.absenceTypes.length + pickedAbsences.value.indexOf(name) + 3) % PALETTE.length],
        reducesTarget: type?.reduces ?? true,
        affectsVacation: false,
      }),
    )
  }
  await call(commands.jurisdictionSet(profileId.value, region.value))
  await work.load()
}

async function finish() {
  saving.value = true
  try {
    await apply()
    close()
  } catch (err) {
    toast.error(t('setup.failed', { error: errorMessage(err) }))
  } finally {
    saving.value = false
  }
}

/** Skip for now: on the first start only the (suggested) country is saved. */
async function skip() {
  if (!work.needsOnboarding) return close()
  saving.value = true
  try {
    await call(commands.jurisdictionSet(profileId.value, region.value))
    close()
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog :open="open">
    <DialogContent
      class="flex max-h-[calc(100svh-2rem)] flex-col gap-5 sm:max-w-2xl"
      :show-close-button="false"
      @escape-key-down.prevent
      @pointer-down-outside.prevent
      @interact-outside.prevent
    >
      <DialogHeader>
        <DialogTitle>{{ $t('setup.title') }}</DialogTitle>
        <DialogDescription>{{ $t('setup.stepOf', { n: step + 1, total: STEPS.length }) }}</DialogDescription>
      </DialogHeader>

      <!-- Stepper: every step shares the width; click a step to jump to it. -->
      <ol class="flex" :aria-label="$t('setup.progress')">
        <li
          v-for="(s, i) in STEPS"
          :key="s"
          class="flex min-w-0 flex-1 flex-col items-center gap-1.5"
          :aria-current="i === step ? 'step' : undefined"
        >
          <div class="flex w-full items-center">
            <span class="h-0.5 flex-1" :class="i === 0 ? 'bg-transparent' : i <= step ? 'bg-primary' : 'bg-border'" />
            <button
              type="button"
              :aria-label="`${i + 1}: ${$t(`setup.${s}.label`)}`"
              class="flex size-7 shrink-0 items-center justify-center rounded-full border text-xs font-semibold transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              :class="i <= step ? 'border-primary bg-primary text-primary-foreground' : 'bg-background text-muted-foreground'"
              @click="step = i"
            >
              <Check v-if="i < step" class="size-3.5" />
              <template v-else>{{ i + 1 }}</template>
            </button>
            <span class="h-0.5 flex-1" :class="i === LAST ? 'bg-transparent' : i < step ? 'bg-primary' : 'bg-border'" />
          </div>
          <span
            class="max-w-full truncate px-0.5 text-[11px] leading-3.5"
            :class="i === step ? 'font-semibold text-foreground' : 'text-muted-foreground'"
          >{{ $t(`setup.${s}.label`) }}</span>
        </li>
      </ol>

      <div class="grid min-h-64 flex-1 content-start gap-4 overflow-y-auto border-t pt-5 pr-1">
        <div>
          <h3 class="font-semibold">{{ $t(`setup.${key}.title`) }}</h3>
          <p class="text-sm text-muted-foreground">{{ $t(`setup.${key}.description`) }}</p>
        </div>

        <!-- 1. You -->
        <div v-if="key === 'you'" class="grid gap-4">
          <CountryPicker v-model:profile-id="profileId" v-model:region="region" />
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('settings.theme') }}</Label>
            <ThemeSwitch />
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('settings.durationFormat') }}</Label>
            <Select v-model="prefs.durationFormat">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="f in durationFormats" :key="f" :value="f">{{ $t(`settings.formats.${f}`) }}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('settings.region.weekStart') }}</Label>
            <Select v-model="prefs.weekStart">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="d in weekStarts" :key="d" :value="d">
                  {{ d === 'auto' ? $t('setup.you.systemDefault') : $t(`settings.region.weekdays.${d}`) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('settings.region.dateFormat') }}</Label>
            <div class="flex flex-wrap gap-2">
              <Button
                v-for="f in dateFormats"
                :key="f.value"
                size="sm"
                :variant="prefs.dateFormat === f.value ? 'default' : 'outline'"
                :aria-pressed="prefs.dateFormat === f.value"
                @click="prefs.dateFormat = f.value"
              >{{ f.label }}</Button>
            </div>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('settings.region.timeFormat') }}</Label>
            <div class="flex flex-wrap gap-2">
              <Button
                v-for="f in timeFormats"
                :key="f.value"
                size="sm"
                :variant="prefs.timeFormat === f.value ? 'default' : 'outline'"
                :aria-pressed="prefs.timeFormat === f.value"
                @click="prefs.timeFormat = f.value"
              >{{ f.label }}</Button>
            </div>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <div>
              <Label>{{ $t('settings.autostart') }}</Label>
              <p class="text-xs text-muted-foreground">{{ $t('settings.autostartHint') }}</p>
            </div>
            <Switch v-model="prefs.autostart" />
          </div>
        </div>

        <!-- 2. Work week -->
        <div v-else-if="key === 'workWeek'" class="grid gap-4">
          <div class="grid gap-2">
            <Label>{{ $t('setup.workWeek.workdays') }}</Label>
            <div class="flex flex-wrap gap-2">
              <Button
                v-for="(on, i) in week.days"
                :key="i"
                size="sm"
                class="min-w-14"
                :variant="on ? 'default' : 'outline'"
                :aria-pressed="on"
                @click="week.days[i] = !on"
              >{{ weekdayLabel(i) }}</Button>
            </div>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label for="setup-hours">{{ $t('setup.workWeek.hours') }}</Label>
            <Input id="setup-hours" v-model.number="week.hours" type="number" min="0" max="24" step="0.5" />
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label for="setup-vacation">{{ $t('setup.workWeek.vacation') }}</Label>
            <Input id="setup-vacation" v-model.number="week.vacation" type="number" min="0" max="366" step="1" />
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <div>
              <Label>{{ $t('setup.workWeek.deductBreaks') }}</Label>
              <p class="text-xs text-muted-foreground">{{ $t('setup.workWeek.deductBreaksHint') }}</p>
            </div>
            <Switch v-model="week.deductBreaks" />
          </div>
        </div>

        <!-- 3. Activities -->
        <div v-else-if="key === 'activities'" class="grid gap-4">
          <p class="text-sm text-muted-foreground">{{ $t('setup.activities.existing', activities.all.length) }}</p>
          <div class="flex flex-wrap gap-2">
            <Button
              v-for="name in suggestions"
              :key="name"
              size="sm"
              :variant="picked.includes(name) ? 'default' : 'outline'"
              :aria-pressed="picked.includes(name)"
              @click="picked = toggle(picked, name)"
            >
              <Check v-if="picked.includes(name)" /><Plus v-else /> {{ name }}
            </Button>
          </div>
          <form class="flex max-w-sm gap-2" @submit.prevent="addCustomActivity">
            <Input v-model="customActivity" :placeholder="$t('setup.activities.placeholder')" :aria-label="$t('setup.activities.placeholder')" />
            <Button type="submit" variant="outline">{{ $t('setup.add') }}</Button>
          </form>
        </div>

        <!-- 4. Projects -->
        <div v-else-if="key === 'projects'" class="grid gap-4">
          <form class="grid gap-3 sm:grid-cols-[1fr_8rem_auto] sm:items-end" @submit.prevent="addProject">
            <div class="grid gap-2">
              <Label for="setup-project">{{ $t('setup.projects.name') }}</Label>
              <Input id="setup-project" v-model="projectForm.name" />
            </div>
            <div class="grid gap-2">
              <Label for="setup-alias">{{ $t('setup.projects.alias') }}</Label>
              <Input id="setup-alias" v-model="projectForm.alias" />
            </div>
            <Button type="submit" variant="outline"><Plus /> {{ $t('setup.add') }}</Button>
          </form>
          <ul v-if="newProjects.length" class="divide-y rounded-lg border text-sm">
            <li v-for="p in newProjects" :key="p.name" class="flex items-center gap-3 px-3 py-2">
              <span class="font-medium">{{ p.name }}</span>
              <span class="flex-1 font-mono text-xs text-muted-foreground">{{ p.alias }}</span>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="newProjects = newProjects.filter((x) => x !== p)"><X /></Button>
            </li>
          </ul>
        </div>

        <!-- 5. Cost centers -->
        <div v-else-if="key === 'costCenters'" class="grid gap-4">
          <form class="grid gap-3 sm:grid-cols-[8rem_1fr_auto] sm:items-end" @submit.prevent="addCostCenter">
            <div class="grid gap-2">
              <Label for="setup-cc-code">{{ $t('costCenters.code') }}</Label>
              <Input id="setup-cc-code" v-model="ccForm.code" />
            </div>
            <div class="grid gap-2">
              <Label for="setup-cc-name">{{ $t('costCenters.name') }}</Label>
              <Input id="setup-cc-name" v-model="ccForm.name" />
            </div>
            <Button type="submit" variant="outline"><Plus /> {{ $t('setup.add') }}</Button>
          </form>
          <ul v-if="newCostCenters.length" class="divide-y rounded-lg border text-sm">
            <li v-for="c in newCostCenters" :key="c.code" class="flex items-center gap-3 px-3 py-2">
              <span class="w-24 shrink-0 font-mono text-xs">{{ c.code }}</span>
              <span class="flex-1">{{ c.name }}</span>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="removeCostCenter(c.code)"><X /></Button>
            </li>
          </ul>
          <div v-if="defaultOptions.length" class="grid gap-2">
            <div>
              <Label>{{ $t('costCenters.myDefault') }}</Label>
              <p class="text-xs text-muted-foreground">{{ $t('costCenters.myDefaultHint') }}</p>
            </div>
            <Select v-model="defaultValue">
              <SelectTrigger class="w-full sm:w-80">
                <SelectValue>
                  <span v-if="defaultLabel" class="truncate">{{ defaultLabel }}</span>
                  <span v-else class="text-muted-foreground">{{ $t('costCenters.choose') }}</span>
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem :value="NONE"><span class="text-muted-foreground">{{ $t('costCenters.choose') }}</span></SelectItem>
                <SelectItem v-for="o in defaultOptions" :key="o.key" :value="o.key">{{ o.label }}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <!-- 6. Absences -->
        <div v-else-if="key === 'absences'" class="grid gap-4">
          <div class="flex flex-wrap gap-2">
            <Button
              v-for="a in absenceOptions"
              :key="a.key"
              size="sm"
              :variant="pickedAbsences.includes(a.name) ? 'default' : 'outline'"
              :aria-pressed="pickedAbsences.includes(a.name)"
              @click="pickedAbsences = toggle(pickedAbsences, a.name)"
            >
              <Check v-if="pickedAbsences.includes(a.name)" /><Plus v-else /> {{ a.name }}
            </Button>
          </div>
          <p v-if="absenceOptions.length === 0" class="text-sm text-muted-foreground">{{ $t('setup.allExist') }}</p>
        </div>

        <!-- 7. Auto-pause -->
        <div v-else-if="key === 'autoPause'" class="grid gap-4">
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('autopause.enabled') }}</Label>
            <Switch v-model="autoPause.enabled" />
          </div>
          <fieldset :disabled="!autoPause.enabled" class="grid gap-4 disabled:opacity-60">
            <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
              <Label for="setup-idle">{{ $t('autopause.idleMinutes') }}</Label>
              <Input id="setup-idle" v-model.number="autoPause.idleMinutes" type="number" min="1" max="240" />
            </div>
            <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
              <Label>{{ $t('autopause.idleAction') }}</Label>
              <Select v-model="autoPause.idleAction">
                <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem v-for="a in awayActions" :key="a" :value="a">{{ $t(`autopause.actions.${a}`) }}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
              <div>
                <Label>{{ $t('autopause.lunch') }}</Label>
                <p class="text-xs text-muted-foreground">{{ $t('autopause.lunchHint') }}</p>
              </div>
              <Switch v-model="autoPause.lunch" />
            </div>
          </fieldset>
        </div>

        <!-- 8. Review -->
        <dl v-else class="divide-y rounded-lg border text-sm">
          <div v-for="row in summary" :key="row.label" class="flex gap-3 px-3 py-2">
            <dt class="w-44 shrink-0 text-muted-foreground">{{ row.label }}</dt>
            <dd class="min-w-0 flex-1">{{ row.value }}</dd>
          </div>
        </dl>
      </div>

      <div class="flex items-center justify-between gap-2">
        <div class="flex gap-2">
          <Button variant="outline" :disabled="step === 0 || saving" @click="step--">{{ $t('setup.back') }}</Button>
          <Button variant="ghost" :disabled="saving" @click="skip">{{ $t('setup.skip') }}</Button>
        </div>
        <Button v-if="step < LAST" @click="step++">{{ $t('setup.next') }}</Button>
        <Button v-else :disabled="saving" @click="finish">{{ saving ? $t('setup.saving') : $t('setup.finish') }}</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
