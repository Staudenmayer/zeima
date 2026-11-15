<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Maximize2, Play } from '@lucide/vue'
import { toast } from 'vue-sonner'
import { commands, type MeetingInput, type MeetingOccurrence, type Room } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { fromInputs, toDateInput, toTimeInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import ActivitySelect from './ActivitySelect.vue'
import MultiSelect from './MultiSelect.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagPicker from './TagPicker.vue'
import TimeInput from './TimeInput.vue'

// A meeting of the person's own, changed right in the popover (calendar views):
// every change is saved at once. "All fields" opens the full meeting dialog.
const props = defineProps<{ occurrence: MeetingOccurrence; canTrack?: boolean }>()
const emit = defineEmits<{ changed: []; more: []; track: [] }>()
const format = useFormat()
const org = useOrgStore()
const account = useAccountStore()

const NO_ROOM = '__none'
const m = computed(() => props.occurrence.meeting)
const form = ref({ title: '', start: '', end: '', place: '', roomId: null as string | null, activityId: null as string | null, projectId: null as string | null, tagIds: [] as string[], attendees: [] as string[], note: '', billable: false })
const timeError = ref(false)
const rooms = ref<Room[]>([])
const people = computed(() => org.people.filter((p) => p.id !== account.status.user?.id))

watch(
  () => props.occurrence,
  (o) => {
    const x = o.meeting
    form.value = {
      title: x.title,
      start: toTimeInput(o.startAt),
      end: toTimeInput(o.endAt),
      place: x.location ?? '',
      roomId: x.roomId,
      activityId: x.activityId,
      projectId: x.projectId,
      tagIds: [...x.tagIds],
      attendees: [...x.attendeeIds],
      note: x.note,
      billable: x.billable,
    }
    timeError.value = false
  },
  { immediate: true },
)
void call(commands.roomsList()).then((r) => (rooms.value = r)).catch(() => {})

/** Saves the meeting with `patch` applied (the times are those of the series, not of this date). */
async function save(patch: Partial<MeetingInput>) {
  const x = m.value
  const input: MeetingInput = {
    title: x.title,
    startAt: x.startAt,
    endAt: x.endAt,
    tz: x.tz,
    location: x.roomId ? null : x.location,
    roomId: x.roomId,
    onlineUrl: x.onlineUrl,
    projectId: x.projectId,
    activityId: x.activityId,
    tagIds: x.tagIds,
    billable: x.billable,
    costCenterId: x.costCenterId,
    note: x.note,
    rrule: x.rrule,
    attendeeIds: x.attendeeIds,
    ...patch,
  }
  try {
    await call(commands.meetingUpdate(x.id, input))
    emit('changed')
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const day = computed(() => toDateInput(props.occurrence.startAt))
/** A time was changed: the series moves by the same amount, so a repeating meeting keeps its dates. */
function saveTimes() {
  const o = props.occurrence
  const a = fromInputs(day.value, form.value.start)
  const b = fromInputs(day.value, form.value.end)
  if (Number.isNaN(a) || Number.isNaN(b) || b <= a) {
    timeError.value = true
    return
  }
  timeError.value = false
  if (a === o.startAt && b === o.endAt) return
  void save({ startAt: m.value.startAt + (a - o.startAt), endAt: m.value.endAt + (b - o.endAt) })
}
watch(() => [form.value.start, form.value.end], saveTimes)
function pickRoom(v: unknown) {
  const id = v === NO_ROOM ? null : String(v)
  form.value.roomId = id
  void save({ roomId: id, location: id ? null : form.value.place || null })
}
function setTags(v: string[]) {
  form.value.tagIds = v
  void save({ tagIds: v })
}
function setProject(v: string | null) {
  form.value.projectId = v
  void save({ projectId: v })
}
function setActivity(v: string | null) {
  form.value.activityId = v
  void save({ activityId: v })
}
function setAttendees(v: string[]) {
  form.value.attendees = v
  void save({ attendeeIds: v })
}
function setBillable(v: boolean) {
  form.value.billable = v
  void save({ billable: v })
}
</script>

<template>
  <div class="grid gap-2.5">
    <div class="flex items-center gap-1">
      <p class="min-w-0 flex-1 truncate text-xs text-muted-foreground tabular-nums">{{ format.day(occurrence.startAt) }}</p>
      <Button v-if="canTrack" type="button" size="icon" variant="ghost" class="size-7" :title="$t('timesheet.trackEvent')" :aria-label="$t('timesheet.trackEvent')" @click="emit('track')"><Play /></Button>
      <Button type="button" size="icon" variant="ghost" class="size-7" :title="$t('timesheet.allFields')" :aria-label="$t('timesheet.allFields')" @click="emit('more')"><Maximize2 /></Button>
    </div>
    <Input v-model="form.title" :aria-label="$t('meetings.title')" maxlength="200" @change="form.title.trim() ? save({ title: form.title.trim() }) : (form.title = m.title)" />
    <ActivitySelect :model-value="form.activityId" @update:model-value="setActivity" />
    <ProjectSelect :model-value="form.projectId" @update:model-value="setProject" />
    <div class="grid grid-cols-[auto_1fr_auto_1fr] items-center gap-x-2 gap-y-1">
      <span class="text-xs text-muted-foreground">{{ $t('meetings.start') }}</span>
      <TimeInput v-model="form.start" :aria-label="$t('meetings.start')" />
      <span class="text-xs text-muted-foreground">{{ $t('meetings.end') }}</span>
      <TimeInput v-model="form.end" :aria-label="$t('meetings.end')" />
    </div>
    <p v-if="timeError" class="text-xs text-destructive">{{ $t('meetings.timeError') }}</p>
    <div class="grid gap-1.5">
      <Label class="text-xs text-muted-foreground">{{ $t('meetings.location') }}</Label>
      <Select v-if="rooms.length" :model-value="form.roomId ?? NO_ROOM" @update:model-value="pickRoom">
        <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem :value="NO_ROOM">{{ $t('meetings.otherPlace') }}</SelectItem>
          <SelectItem v-for="r in rooms" :key="r.id" :value="r.id">{{ r.name }}</SelectItem>
        </SelectContent>
      </Select>
      <Input
        v-if="!form.roomId"
        v-model="form.place"
        maxlength="200"
        :placeholder="$t('meetings.otherPlacePlaceholder')"
        :aria-label="$t('meetings.location')"
        @change="save({ location: form.place.trim() || null })"
      />
    </div>
    <div class="grid gap-1.5">
      <Label class="text-xs text-muted-foreground">{{ $t('meetings.attendees') }}</Label>
      <MultiSelect
        :model-value="form.attendees"
        :options="people.map((p) => ({ value: p.id, label: p.name || $t('team.someone'), person: true }))"
        :placeholder="$t('meetings.addAttendees')"
        :empty="$t('meetings.noPeople')"
        :search-placeholder="$t('directory.search')"
        @update:model-value="setAttendees"
      />
    </div>
    <TagPicker :model-value="form.tagIds" :project-id="form.projectId" @update:model-value="setTags" />
    <Textarea v-model="form.note" rows="2" :placeholder="$t('meetings.note')" :aria-label="$t('meetings.note')" @change="save({ note: form.note })" />
    <label class="flex items-center gap-2 text-xs">
      <Switch :model-value="form.billable" @update:model-value="setBillable" />
      {{ $t('entry.billable') }}
    </label>
  </div>
</template>
