<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands, type AddonTimelineItem, type Meeting, type MeetingInput, type Room } from '@/bindings'
import { DoorOpen, Play, X } from '@lucide/vue'
import RoomsDialog from './RoomsDialog.vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { call, errorMessage } from '@/lib/ipc'
import { fromInputs, getTimeZone, toDateInput, toTimeInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useCostCentersStore } from '@/stores/costCenters'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'
import ActivitySelect from './ActivitySelect.vue'
import DateInput from './DateInput.vue'
import AttendeePanel from './AttendeePanel.vue'
import NoteEditor from './NoteEditor.vue'
import PlaceInput from './PlaceInput.vue'
import TimeInput from './TimeInput.vue'
import CostCenterSelect from './CostCenterSelect.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagInput from './TagInput.vue'

const props = defineProps<{ meeting: Meeting | null; /** Times for a new meeting (from the timesheet's add dialog or a drag). */ draft?: { startAt: number; endAt: number } | null; /** The meeting as a timeline item: the dialog then offers to track it. */ trackItem?: AddonTimelineItem | null }>()
const emit = defineEmits<{ saved: []; track: [item: AddonTimelineItem]; /** "Time entry" was chosen on a new meeting: the times so far, for the entry dialog. */ entry: [times: { startAt: number; endAt: number }] }>()
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

const form = ref({ title: '', date: '', endDate: '', start: '09:00', end: '10:00', location: '', roomId: null as string | null, url: '', note: '', projectId: null as string | null, activityId: null as string | null, tagIds: [] as string[], billable: false, costCenterId: null as string | null, attendees: [] as string[], repeat: 'none' as Repeat, weekdays: [] as number[], count: 10 })
const saving = ref(false)
const roomsOpen = ref(false)
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
  const d = m ? null : (props.draft ?? null)
  const day = m ? toDateInput(m.startAt) : d ? toDateInput(d.startAt) : toDateInput(Date.now() + 86_400_000)
  form.value = {
    title: m?.title ?? '',
    date: day,
    endDate: m ? toDateInput(m.endAt) : d ? toDateInput(d.endAt) : day,
    start: m ? toTimeInput(m.startAt) : d ? toTimeInput(d.startAt) : '09:00',
    end: m ? toTimeInput(m.endAt) : d ? toTimeInput(d.endAt) : '10:00',
    location: m?.location ?? '',
    roomId: m?.roomId ?? null,
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

// The end date follows the start date until it is set apart from it.
watch(
  () => form.value.date,
  (date, old) => {
    if (!form.value.endDate || form.value.endDate === old || form.value.endDate < date) form.value.endDate = date
  },
)

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
/** A repeating meeting keeps its length when it starts on a later day. */
const endAt = computed(() => startAt.value + (fromInputs(form.value.endDate || form.value.date, form.value.end) - fromInputs(form.value.date, form.value.start)))

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

// The rooms of the organization, and which are booked at the chosen time
// (asked of the server, which knows every meeting; only times come back).
const rooms = ref<Room[]>([])
const roomBooked = ref<Set<string>>(new Set())
let roomTicket = 0
watch(
  [open, startAt, endAt, roomsOpen],
  async () => {
    if (!open.value) return
    const mine = ++roomTicket
    try {
      rooms.value = await call(commands.roomsList())
      const orgId = workspaces.active?.orgId
      if (!orgId || rooms.value.length === 0) {
        roomBooked.value = new Set()
        return
      }
      const text = await call(
        commands.orgRequest('POST', `/v1/orgs/${orgId}/rooms/bookings`, JSON.stringify({ from: startAt.value, to: endAt.value > startAt.value ? endAt.value : startAt.value + 3_600_000, ...(props.meeting ? { exclude: props.meeting.id } : {}) })),
      )
      if (mine === roomTicket) roomBooked.value = new Set(Object.keys(JSON.parse(text || '{}')))
    } catch {
      if (mine === roomTicket) roomBooked.value = new Set()
    }
  },
  { immediate: true },
)

/** The people who are busy at the chosen time, and when. */
const conflicts = computed(() =>
  form.value.attendees
    .map((id) => ({ id, times: (busy.value[id] ?? []).filter((b) => b.from < endAt.value && b.to > startAt.value) }))
    .filter((c) => c.times.length > 0),
)
const clock = (ms: number) => toTimeInput(ms)

/** A new meeting can become a time entry again: the switch hands the times over to the entry dialog. */
function toEntry() {
  const start = startAt.value
  const end = endAt.value > start ? endAt.value : start + 3_600_000
  open.value = false
  emit('entry', { startAt: Number.isNaN(start) ? Date.now() : start, endAt: end })
}

async function save() {
  const f = form.value
  if (f.roomId && roomBooked.value.has(f.roomId)) {
    toast.error(t('meetings.roomTaken'))
    return
  }
  if (f.repeat === 'weekly' && f.weekdays.length === 0) {
    toast.error(t('meetings.pickWeekday'))
    return
  }
  const input: MeetingInput = {
    title: f.title,
    startAt: startAt.value,
    endAt: endAt.value > startAt.value ? endAt.value : startAt.value + 3_600_000,
    tz: getTimeZone() ?? null,
    location: f.roomId ? null : f.location || null,
    roomId: f.roomId,
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
    <DialogContent :aria-describedby="undefined" :show-close-button="false" class="h-[min(44rem,94vh)] grid-rows-[auto_1fr] overflow-hidden sm:max-w-6xl">
      <!-- The line before the attendees runs the whole height of the dialog. -->
      <div class="pointer-events-none absolute inset-y-0 right-[19.5rem] w-px bg-border" aria-hidden="true" />
      <!-- Title, track and close on one line (the dialog's own close button is off so they line up). -->
      <DialogHeader class="flex-row items-center gap-1">
        <DialogTitle class="min-w-0 flex-1 truncate">{{ meeting ? $t('meetings.edit') : $t('meetings.new') }}</DialogTitle>
        <Button v-if="meeting && trackItem" type="button" size="icon" variant="ghost" class="size-7" :title="$t('timesheet.trackEvent')" :aria-label="$t('timesheet.trackEvent')" @click="(open = false, emit('track', trackItem))"><Play /></Button>
        <Button type="button" size="icon" variant="ghost" class="size-7" :title="$t('common.close')" :aria-label="$t('common.close')" @click="open = false"><X /></Button>
      </DialogHeader>
      <form class="flex min-h-0 flex-col" @submit.prevent="save">
        <div class="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_18rem]">
          <div class="flex min-h-0 min-w-0 flex-col gap-4 overflow-y-auto px-1 pr-8 pb-4 -mx-1">
            <div v-if="!meeting" role="group" :aria-label="$t('timesheet.whatToAdd')" class="inline-flex gap-0.5 self-start rounded-lg bg-muted p-0.5">
              <Button type="button" size="sm" variant="ghost" class="h-7" aria-pressed="false" @click="toEntry">{{ $t('timesheet.addKindEntry') }}</Button>
              <Button type="button" size="sm" variant="outline" class="h-7" aria-pressed="true">{{ $t('timesheet.addKindMeeting') }}</Button>
            </div>
            <div class="grid gap-2"><Label for="m-title">{{ $t('meetings.title') }}</Label><Input id="m-title" v-model="form.title" required maxlength="200" autofocus /></div>
            <div class="grid grid-cols-4 gap-3">
              <div class="grid gap-2"><Label for="m-date">{{ $t('entry.startDate') }}</Label><DateInput id="m-date" v-model="form.date" required /></div>
              <div class="grid gap-2"><Label for="m-start">{{ $t('entry.startTime') }}</Label><TimeInput id="m-start" v-model="form.start" required /></div>
              <div class="grid gap-2"><Label for="m-end-date">{{ $t('entry.endDate') }}</Label><DateInput id="m-end-date" v-model="form.endDate" required /></div>
              <div class="grid gap-2"><Label for="m-end">{{ $t('entry.endTime') }}</Label><TimeInput id="m-end" v-model="form.end" required /></div>
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
            <div class="grid grid-cols-2 gap-3">
              <div class="grid gap-2"><Label>{{ $t('meetings.activity') }}</Label><ActivitySelect v-model="form.activityId" /></div>
              <div class="grid gap-2"><Label>{{ $t('meetings.project') }}</Label><ProjectSelect v-model="form.projectId" /></div>
            </div>
            <div class="grid grid-cols-4 items-end gap-3">
              <div class="col-span-2 grid gap-2">
                <Label>{{ $t('entry.tags') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
                <TagInput v-model="form.tagIds" :project-id="form.projectId" />
              </div>
              <div v-if="costCenters.all.length" class="grid gap-2">
                <Label>{{ $t('costCenters.costCenter') }}</Label>
                <CostCenterSelect v-model="form.costCenterId" :empty-label="$t('costCenters.automatic')" />
              </div>
              <label class="flex h-9 items-center gap-2 text-sm">
                <Switch v-model="form.billable" />
                {{ $t('entry.billable') }}
              </label>
            </div>
            <!-- Place and online link side by side; the room warning goes under both. -->
            <div class="grid grid-cols-2 gap-3">
              <div class="grid gap-2">
                <div class="flex items-center justify-between gap-2">
                  <Label for="m-loc">{{ $t('meetings.location') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
                  <button v-if="workspaces.can('policy.write')" type="button" class="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground" @click="roomsOpen = true"><DoorOpen class="size-3" /> {{ $t('meetings.rooms') }}</button>
                </div>
                <PlaceInput id="m-loc" v-model:room-id="form.roomId" v-model:location="form.location" :rooms="rooms" :booked="roomBooked" />
              </div>
              <div class="grid gap-2"><Label for="m-url">{{ $t('meetings.onlineUrl') }}</Label><Input id="m-url" v-model="form.url" type="url" placeholder="https://" /></div>
              <p v-if="form.roomId && roomBooked.has(form.roomId)" class="col-span-2 -mt-1 text-xs text-amber-600">{{ $t('meetings.roomTaken') }}</p>
            </div>
            <div class="grid gap-2"><Label for="m-note">{{ $t('meetings.note') }}</Label><NoteEditor id="m-note" v-model="form.note" min-height="6rem" /></div>
          </div>
          <div class="flex min-h-0 flex-col gap-2 pl-8">
            <AttendeePanel v-model="form.attendees" :people="others" />
            <p v-for="c in conflicts" :key="c.id" class="text-xs text-amber-600">
              {{ $t('meetings.busy', { name: nameOf(c.id), times: c.times.map((t) => (t.kind === 'absence' ? $t('meetings.absent') : `${clock(t.from)}–${clock(t.to)}`)).join(', ') }) }}
            </p>
            <DialogFooter class="mt-auto pt-2">
              <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
              <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
            </DialogFooter>
          </div>
        </div>
      </form>
    </DialogContent>
  </Dialog>
  <RoomsDialog v-model:open="roomsOpen" />
</template>
