<script setup lang="ts">
import { computed, watch } from 'vue'
import { Copy, Lock, Maximize2, Play, Scissors, Trash2 } from '@lucide/vue'
import type { TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useEntryForm } from '@/composables/useEntryForm'
import type { Anchor } from '@/lib/anchor'
import { useFormat } from '@/composables/useFormat'
import { parseDate } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'
import { useTimerStore } from '@/stores/timer'
import ActivitySelect from './ActivitySelect.vue'
import DateInput from './DateInput.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagPicker from './TagPicker.vue'
import TimeInput from './TimeInput.vue'

// Compact editor next to a calendar block, like a context menu: the common
// fields and actions; everything else is in the full dialog ("more").
// Changes are saved as they are made; there is no save button.

const props = defineProps<{
  entry: TimeEntry | null
  /** Where the popover is placed (next to the clicked block). */
  reference: Anchor | null
  /** Live times while the entry's edges are dragged in the calendar. */
  times?: { startAt: number; endAt: number | null } | null
}>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{
  continue: [entry: TimeEntry]
  duplicate: [entry: TimeEntry]
  split: [entry: TimeEntry]
  delete: [entry: TimeEntry]
  /** Open the full entry dialog. */
  more: [entry: TimeEntry]
}>()

const format = useFormat()
const settings = useSettingsStore()
const timer = useTimerStore()

const {
  activityId, activityMissing, projectId, tagIds, date, endDate, start, end, note, billable, saving, running,
  timeInvalid, changed, reset, setTimes, save: saveForm,
} = useEntryForm(() => props.entry)

/** A change came in while saving; save again afterwards. */
let again = false

/** Spans several days: the dates are shown with the times. */
const multiDay = computed(() => !running.value && endDate.value !== date.value)
const shortDay = (iso: string) => {
  const ms = parseDate(iso)
  return Number.isFinite(ms) ? `${format.weekday(ms)} ${format.shortDate(ms)}` : ''
}

watch(
  [open, () => props.entry?.id],
  ([isOpen]) => {
    if (isOpen) reset()
  },
  { immediate: true },
)

// Dragged or saved times (calendar) update only the dates and times; other
// unsaved changes in the form stay.
watch(
  () => {
    const t = props.times ?? props.entry
    return t ? [t.startAt, t.endAt] as const : null
  },
  (t) => {
    // While an autosave runs or is queued, the reload must not overwrite
    // times typed meanwhile; a drag (`times`) always applies.
    if (t && open.value && (props.times || (!saving.value && !again))) setTimes(t[0], t[1])
  },
)

/** Kept inside the calendar, so it never covers the sidebar. */
const boundary = computed(() => props.reference?.contextElement.closest('[data-calendar]') ?? null)

/** Pressing the entry's own block (e.g. to drag its edges) keeps it open. */
function keepOpenOnBlock(event: CustomEvent<{ originalEvent: Event }>) {
  const target = event.detail.originalEvent.target
  if (target instanceof Node && props.reference?.contextElement.contains(target)) event.preventDefault()
}

const durationMs = computed(() => (props.entry ? (props.entry.endAt ?? timer.now) - props.entry.startAt : 0))

// Autosave. Pickers and toggles save at once, the note after a pause in
// typing, times when their field is left. Nothing is saved while the entry
// is dragged in the calendar (the drag saves itself) or while the form is
// invalid (shown inline instead).
const NOTE_PAUSE_MS = 600
let noteTimer: ReturnType<typeof setTimeout> | undefined

async function autosave() {
  clearTimeout(noteTimer)
  if (props.times || props.entry?.locked || !changed()) return
  // One save at a time; changes made meanwhile are saved right after.
  if (saving.value) {
    again = true
    return
  }
  await saveForm()
  if (again) {
    again = false
    void autosave()
  }
}

watch([activityId, projectId, tagIds, date, endDate, start, end, billable], () => void autosave(), { deep: true })
watch(note, () => {
  clearTimeout(noteTimer)
  noteTimer = setTimeout(() => void autosave(), NOTE_PAUSE_MS)
})
// Closing saves a note that was still being typed.
watch(open, (isOpen) => {
  if (!isOpen) void autosave()
})

/** Enter in a field: save now and close. */
async function submit() {
  await autosave()
  open.value = false
}

/** Closes the popover, then hands the action to the page. */
function act(action: 'continue' | 'duplicate' | 'split' | 'delete' | 'more') {
  const e = props.entry
  if (!e) return
  open.value = false
  // Typed per event name; all take the entry.
  ;(emit as (name: typeof action, entry: TimeEntry) => void)(action, e)
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverAnchor :reference="reference ?? undefined" />
    <PopoverContent
      v-if="entry"
      side="right"
      align="start"
      :side-offset="6"
      :collision-padding="8"
      :collision-boundary="boundary"
      class="max-h-(--reka-popover-content-available-height) w-80 overflow-x-hidden overflow-y-auto p-3"
      @open-auto-focus.prevent
      @interact-outside="keepOpenOnBlock"
    >
      <!-- `min-w-0` everywhere: the card may get narrower than w-80 when
           little room is left next to the event, and must not overflow. -->
      <form class="grid min-w-0 grid-cols-1 gap-2.5 text-sm [&>*]:min-w-0" @submit.prevent="submit">
        <!-- When, and the quick actions -->
        <div class="flex items-center gap-1">
          <div class="min-w-0 flex-1 text-xs text-muted-foreground">
            <div class="truncate">
              {{ entry.endAt !== null && multiDay ? `${shortDay(date)} – ${shortDay(endDate)}` : format.day(entry.startAt) }}
            </div>
            <div class="tabular-nums">{{ settings.formatDuration(durationMs) }} h</div>
          </div>
          <Lock v-if="entry.locked" class="size-3.5 text-muted-foreground" :aria-label="$t('timesheet.locked')" />
          <Button
            v-if="entry.endAt !== null"
            type="button" size="icon" variant="ghost" class="size-7"
            :title="$t('timer.continue')" :aria-label="$t('timer.continue')"
            @click="act('continue')"
          ><Play /></Button>
          <Button
            v-if="entry.endAt !== null"
            type="button" size="icon" variant="ghost" class="size-7"
            :title="$t('timesheet.duplicate')" :aria-label="$t('timesheet.duplicate')"
            @click="act('duplicate')"
          ><Copy /></Button>
          <Button
            type="button" size="icon" variant="ghost" class="size-7" :disabled="entry.locked"
            :title="$t('timesheet.split')" :aria-label="$t('timesheet.split')"
            @click="act('split')"
          ><Scissors /></Button>
          <Button
            type="button" size="icon" variant="ghost" class="size-7" :disabled="entry.locked"
            :title="$t('common.delete')" :aria-label="$t('common.delete')"
            @click="act('delete')"
          ><Trash2 /></Button>
          <Button
            type="button" size="icon" variant="ghost" class="size-7"
            :title="$t('timesheet.allFields')" :aria-label="$t('timesheet.allFields')"
            @click="act('more')"
          ><Maximize2 /></Button>
        </div>

        <ActivitySelect v-model="activityId" :invalid="activityMissing" />
        <ProjectSelect v-model="projectId" include-archived />
        <!-- Start and end each with a date: entries may span several days. -->
        <div class="grid grid-cols-[auto_minmax(0,1fr)_minmax(0,5.5rem)] items-center gap-x-2 gap-y-1.5">
          <span class="text-xs text-muted-foreground">{{ $t('entry.start') }}</span>
          <DateInput v-model="date" class="min-w-0" required :aria-label="$t('entry.start')" />
          <TimeInput v-model="start" class="min-w-0" required :aria-label="$t('entry.start')" />
          <span class="text-xs text-muted-foreground">{{ $t('entry.end') }}</span>
          <template v-if="!running">
            <DateInput v-model="endDate" class="min-w-0" required :min="date" :aria-label="$t('entry.end')" />
            <TimeInput v-model="end" class="min-w-0" required :aria-label="$t('entry.end')" />
          </template>
          <span v-else class="col-span-2 text-sm text-muted-foreground">{{ $t('timesheet.running') }}</span>
        </div>
        <p v-if="timeInvalid" class="text-xs text-destructive">{{ $t('entry.invalidTime') }}</p>
        <TagPicker v-model="tagIds" :project-id="projectId" />
        <Textarea v-model="note" :placeholder="$t('entry.notePlaceholder')" rows="2" class="min-h-14 text-sm" />

        <label class="flex items-center gap-2 text-xs">
          <Switch v-model="billable" />
          {{ $t('entry.billable') }}
        </label>
      </form>
    </PopoverContent>
  </Popover>
</template>
