<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands, type Meeting, type MeetingInput } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { call, errorMessage } from '@/lib/ipc'
import { fromInputs, getTimeZone, toDateInput, toTimeInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useCostCentersStore } from '@/stores/costCenters'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'
import ActivitySelect from './ActivitySelect.vue'
import CostCenterSelect from './CostCenterSelect.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagPicker from './TagPicker.vue'

const props = defineProps<{ meeting: Meeting | null }>()
const emit = defineEmits<{ saved: [] }>()
const open = defineModel<boolean>('open', { required: true })

const { t } = useI18n()
const account = useAccountStore()
const org = useOrgStore()
const costCenters = useCostCentersStore()
const projects = useProjectsStore()
const workspaces = useWorkspaceStore()

type Repeat = 'none' | 'daily' | 'weekly'
const REPEATS: Repeat[] = ['none', 'daily', 'weekly']
const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU']

const form = ref({ title: '', date: '', start: '09:00', end: '10:00', location: '', url: '', note: '', projectId: null as string | null, activityId: null as string | null, tagIds: [] as string[], billable: false, costCenterId: null as string | null, attendees: [] as string[], repeat: 'none' as Repeat, weekdays: [] as number[], count: 10 })
const saving = ref(false)
const busy = ref<Record<string, { from: number; to: number; kind: string }[]>>({})

const me = computed(() => account.status.user?.id ?? '')
const others = computed(() => org.people.filter((p) => p.id !== me.value))
const nameOf = (id: string) => org.nameOf(id) || id

/** Weekday of a date, 0 = Monday. */
const weekdayOf = (date: string) => (new Date(`${date}T12:00:00`).getDay() + 6) % 7
/** The first date on or after `date` that falls on one of the weekdays. */
function firstOn(date: string, weekdays: number[]): string {
  if (weekdays.length === 0) return date
  const d = new Date(`${date}T12:00:00`)
  for (let i = 0; i < 7 && !weekdays.includes(weekdayOf(toDateInput(d.getTime()))); i++) d.setDate(d.getDate() + 1)
  return toDateInput(d.getTime())
}
/** Names of the weekdays in the person's language, Monday first. */
const dayNames = DAYS.map((_, i) => new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: 'short' }))
/** The weekdays are the person's own choice once they have touched them; until then they follow the date. */
let weekdaysTouched = false
function toggleDay(i: number) {
  weekdaysTouched = true
  const set = new Set(form.value.weekdays)
  if (set.has(i)) set.delete(i)
  else set.add(i)
  form.value.weekdays = [...set].sort()
}

/** The repeat rule for the chosen option. */
function rule(): string | null {
  const { repeat, count, weekdays } = form.value
  if (repeat === 'none') return null
  const n = count > 0 ? `;COUNT=${Math.round(count)}` : ''
  if (repeat === 'weekly') return `FREQ=WEEKLY;BYDAY=${[...weekdays].sort().map((i) => DAYS[i]).join(',')}${n}`
  return `FREQ=${repeat.toUpperCase()}${n}`
}
function repeatOf(rrule: string | null): Repeat {
  if (!rrule) return 'none'
  const f = /FREQ=(\w+)/.exec(rrule)?.[1]?.toLowerCase()
  return f === 'daily' || f === 'weekly' ? f : 'none'
}
function weekdaysOf(rrule: string | null, date: string): number[] {
  const list = /BYDAY=([A-Z,]+)/.exec(rrule ?? '')?.[1]?.split(',').map((d) => DAYS.indexOf(d)).filter((i) => i >= 0)
  return list?.length ? list : [weekdayOf(date)]
}

// Until the weekdays are chosen by hand, a weekly meeting repeats on the weekday of its date.
watch(
  () => form.value.date,
  (date) => {
    if (!weekdaysTouched && date) form.value.weekdays = [weekdayOf(date)]
  },
)

watch(open, async (isOpen) => {
  if (!isOpen) return
  weekdaysTouched = props.meeting !== null
  const m = props.meeting
  const day = m ? toDateInput(m.startAt) : toDateInput(Date.now() + 86_400_000)
  form.value = {
    title: m?.title ?? '',
    date: day,
    start: m ? toTimeInput(m.startAt) : '09:00',
    end: m ? toTimeInput(m.endAt) : '10:00',
    location: m?.location ?? '',
    url: m?.onlineUrl ?? '',
    note: m?.note ?? '',
    projectId: m?.projectId ?? null,
    activityId: m?.activityId ?? null,
    tagIds: m ? [...m.tagIds] : [],
    billable: m?.billable ?? false,
    costCenterId: m?.costCenterId ?? null,
    attendees: m ? [...m.attendeeIds] : [],
    repeat: repeatOf(m?.rrule ?? null),
    weekdays: weekdaysOf(m?.rrule ?? null, day),
    count: Number(/COUNT=(\d+)/.exec(m?.rrule ?? '')?.[1] ?? 10),
  }
  if (org.people.length === 0 && workspaces.active?.orgId) await org.loadGlance(workspaces.active.orgId).catch(() => {})
})

// A new meeting takes the project's billable default, as a new entry does.
watch(
  () => form.value.projectId,
  (id) => {
    if (props.meeting) return
    form.value.billable = id ? (projects.byId.get(id)?.billableDefault ?? false) : false
  },
)

/** The day of the first meeting: a weekly one starts on the first chosen weekday from the date on. */
const firstDate = computed(() => (form.value.repeat === 'weekly' ? firstOn(form.value.date, form.value.weekdays) : form.value.date))
const startAt = computed(() => fromInputs(firstDate.value, form.value.start))
const endAt = computed(() => fromInputs(firstDate.value, form.value.end))

// Who is busy on the chosen day.
let ticket = 0
watch(
  () => [open.value, form.value.date, [...form.value.attendees].join(',')],
  async () => {
    const orgId = workspaces.active?.orgId
    const day = form.value.date
    if (!open.value || !orgId || !day || form.value.attendees.length === 0) {
      busy.value = {}
      return
    }
    const mine = ++ticket
    const from = fromInputs(day, '00:00')
    try {
      const text = await call(
        commands.orgRequest('POST', `/v1/orgs/${orgId}/freebusy`, JSON.stringify({ userIds: form.value.attendees, from, to: from + 86_400_000, timezone: getTimeZone() })),
      )
      if (mine === ticket && text) busy.value = JSON.parse(text)
    } catch {
      busy.value = {}
    }
  },
)

/** The people who are busy at the chosen time, and when. */
const conflicts = computed(() =>
  form.value.attendees
    .map((id) => ({ id, times: (busy.value[id] ?? []).filter((b) => b.from < endAt.value && b.to > startAt.value) }))
    .filter((c) => c.times.length > 0),
)
const clock = (ms: number) => toTimeInput(ms)

function toggle(id: string, on: boolean) {
  const set = new Set(form.value.attendees)
  if (on) set.add(id)
  else set.delete(id)
  form.value.attendees = [...set]
}

async function save() {
  const f = form.value
  if (f.repeat === 'weekly' && f.weekdays.length === 0) {
    toast.error(t('meetings.pickWeekday'))
    return
  }
  const input: MeetingInput = {
    title: f.title,
    startAt: startAt.value,
    endAt: endAt.value > startAt.value ? endAt.value : startAt.value + 3_600_000,
    tz: getTimeZone() ?? null,
    location: f.location || null,
    onlineUrl: f.url || null,
    projectId: f.projectId,
    activityId: f.activityId,
    tagIds: f.tagIds,
    billable: f.billable,
    costCenterId: f.costCenterId,
    note: f.note,
    rrule: rule(),
    attendeeIds: f.attendees,
  }
  saving.value = true
  try {
    if (props.meeting) await call(commands.meetingUpdate(props.meeting.id, input))
    else await call(commands.meetingCreate(input))
    open.value = false
    emit('saved')
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="max-h-[90vh] overflow-y-auto sm:max-w-xl">
      <DialogHeader><DialogTitle>{{ meeting ? $t('meetings.edit') : $t('meetings.new') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-2"><Label for="m-title">{{ $t('meetings.title') }}</Label><Input id="m-title" v-model="form.title" required maxlength="200" autofocus /></div>
        <div class="grid grid-cols-3 gap-3">
          <div class="grid gap-2"><Label for="m-date">{{ $t('meetings.date') }}</Label><Input id="m-date" v-model="form.date" type="date" required /></div>
          <div class="grid gap-2"><Label for="m-start">{{ $t('meetings.start') }}</Label><Input id="m-start" v-model="form.start" type="time" required /></div>
          <div class="grid gap-2"><Label for="m-end">{{ $t('meetings.end') }}</Label><Input id="m-end" v-model="form.end" type="time" required /></div>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2">
            <Label>{{ $t('meetings.repeat') }}</Label>
            <Select v-model="form.repeat">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem v-for="r in REPEATS" :key="r" :value="r">{{ $t(`meetings.repeats.${r}`) }}</SelectItem></SelectContent>
            </Select>
          </div>
          <div v-if="form.repeat !== 'none'" class="grid gap-2"><Label for="m-count">{{ $t('meetings.times') }}</Label><Input id="m-count" v-model.number="form.count" type="number" min="1" max="500" /></div>
        </div>
        <div v-if="form.repeat === 'weekly'" class="grid gap-2">
          <Label>{{ $t('meetings.weekdays') }}</Label>
          <div class="flex flex-wrap gap-1.5" role="group" :aria-label="$t('meetings.weekdays')">
            <Button
              v-for="(name, i) in dayNames"
              :key="name"
              type="button"
              size="sm"
              class="min-w-12"
              :variant="form.weekdays.includes(i) ? 'default' : 'outline'"
              :aria-pressed="form.weekdays.includes(i)"
              @click="toggleDay(i)"
            >{{ name }}</Button>
          </div>
          <p class="text-xs text-muted-foreground">{{ $t('meetings.weeklyHint', { time: `${form.start}–${form.end}` }) }}</p>
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('meetings.attendees') }}</Label>
          <div class="grid max-h-36 gap-1.5 overflow-y-auto rounded-md border p-2">
            <label v-for="p in others" :key="p.id" class="flex items-center gap-2 text-sm">
              <Checkbox :model-value="form.attendees.includes(p.id)" @update:model-value="toggle(p.id, $event === true)" />
              {{ p.name || $t('team.someone') }}
            </label>
            <p v-if="others.length === 0" class="text-sm text-muted-foreground">{{ $t('meetings.noPeople') }}</p>
          </div>
          <p v-for="c in conflicts" :key="c.id" class="text-xs text-amber-600">
            {{ $t('meetings.busy', { name: nameOf(c.id), times: c.times.map((t) => (t.kind === 'absence' ? $t('meetings.absent') : `${clock(t.from)}–${clock(t.to)}`)).join(', ') }) }}
          </p>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2"><Label for="m-loc">{{ $t('meetings.location') }}</Label><Input id="m-loc" v-model="form.location" maxlength="200" /></div>
          <div class="grid gap-2"><Label for="m-url">{{ $t('meetings.onlineUrl') }}</Label><Input id="m-url" v-model="form.url" type="url" placeholder="https://" /></div>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2"><Label>{{ $t('meetings.activity') }}</Label><ActivitySelect v-model="form.activityId" /></div>
          <div class="grid gap-2"><Label>{{ $t('meetings.project') }}</Label><ProjectSelect v-model="form.projectId" /></div>
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('entry.tags') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <TagPicker v-model="form.tagIds" :project-id="form.projectId" />
        </div>
        <div v-if="costCenters.all.length" class="grid gap-2">
          <Label>{{ $t('costCenters.costCenter') }}</Label>
          <CostCenterSelect v-model="form.costCenterId" :empty-label="$t('costCenters.automatic')" />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <Switch v-model="form.billable" />
          {{ $t('entry.billable') }}
        </label>
        <div class="grid gap-2"><Label for="m-note">{{ $t('meetings.note') }}</Label><Textarea id="m-note" v-model="form.note" rows="3" /></div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
