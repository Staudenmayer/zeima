<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { DialogFooter } from '@/components/ui/dialog'
import TimeInput from '@/components/time/TimeInput.vue'
import DateInput from '@/components/time/DateInput.vue'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { type EntryDraft, useEntryForm } from '@/composables/useEntryForm'
import { useActivitiesStore } from '@/stores/activities'
import { useCostCentersStore } from '@/stores/costCenters'
import { fromInputs, toDateInput, toTimeInput } from '@/lib/time'
import ActivitySelect from './ActivitySelect.vue'
import CostCenterSelect from './CostCenterSelect.vue'
import ProjectSelect from './ProjectSelect.vue'
import NoteEditor from './NoteEditor.vue'
import TagInput from './TagInput.vue'

const props = defineProps<{
  /** Room is kept free at the top for the dialog's type switch. */
  lead?: boolean
  /** Entry to edit; `null` creates a new one. */
  entry: TimeEntry | null
  /** Day preselected for new entries (unix ms). */
  day?: number
  /** Values for a new entry, e.g. from a calendar event. */
  draft?: EntryDraft | null
  /** Combine: the entries this one replaces; the form starts with their values (`draft`). */
  merge?: TimeEntry[] | null
}>()
const emit = defineEmits<{ saved: []; /** "Meeting" was chosen: the times so far, for the meeting dialog. */ meeting: [times: { startAt: number; endAt: number }] }>()
const open = defineModel<boolean>('open', { required: true })

const activities = useActivitiesStore()
const costCenters = useCostCentersStore()

const { activityId, activityMissing, projectId, tagIds, date, endDate, start, end, note, billable, costCenterId, saving, running, reset, setCostCenter, save: saveForm } =
  useEntryForm(() => props.entry, () => props.day, () => props.draft ?? null, () => props.merge?.map((e) => e.id) ?? null)

/** Combine: which of the entries' notes go into the new one; the editor below is the preview and can be edited. */
const noteChoices = computed(() => {
  const seen = new Set<string>()
  return [...(props.merge ?? [])]
    .sort((a, b) => a.startAt - b.startAt)
    .filter((e) => {
      const text = e.note.trim()
      if (!text || seen.has(text)) return false
      seen.add(text)
      return true
    })
    .map((e) => ({ id: e.id, text: e.note.trim(), when: `${toDateInput(e.startAt)} ${toTimeInput(e.startAt)}` }))
})
const chosenNotes = ref<string[]>([])
function toggleNote(id: string, on: boolean) {
  chosenNotes.value = on ? [...chosenNotes.value, id] : chosenNotes.value.filter((x) => x !== id)
  note.value = noteChoices.value.filter((c) => chosenNotes.value.includes(c.id)).map((c) => c.text).join('\n\n')
}

watch(
  open,
  (isOpen) => {
    if (!isOpen) return
    reset()
    chosenNotes.value = noteChoices.value.map((c) => c.id)
  },
  { immediate: true },
)

/** In an organization a new item can be a meeting too: the switch hands the times over to the meeting dialog. */
function toMeeting() {
  const startAt = fromInputs(date.value, start.value)
  let endAt = fromInputs(date.value, end.value)
  if (Number.isNaN(endAt) || endAt <= startAt) endAt = startAt + 3_600_000
  open.value = false
  emit('meeting', { startAt: Number.isNaN(startAt) ? Date.now() : startAt, endAt })
}

defineExpose({ toMeeting })

async function save() {
  if (await saveForm()) {
    open.value = false
    emit('saved')
  }
}
</script>

<template>
  <form class="flex h-full flex-col gap-4 overflow-y-auto px-1 -mx-1" :class="lead ? 'pt-[4.375rem]' : ''" @submit.prevent="save">
        <!-- Start and end each have a date (entries may span several days), then activity and project,
             tags, cost center and billable, and the note. -->
        <div class="grid grid-cols-4 gap-3">
          <div class="grid gap-2">
            <Label for="entry-date">{{ $t('entry.startDate') }}</Label>
            <DateInput id="entry-date" v-model="date" required />
          </div>
          <div class="grid gap-2">
            <Label for="entry-start">{{ $t('entry.startTime') }}</Label>
            <TimeInput id="entry-start" v-model="start" required />
          </div>
          <template v-if="!running">
            <div class="grid gap-2">
              <Label for="entry-end-date">{{ $t('entry.endDate') }}</Label>
              <DateInput id="entry-end-date" v-model="endDate" required />
            </div>
            <div class="grid gap-2">
              <Label for="entry-end">{{ $t('entry.endTime') }}</Label>
              <TimeInput id="entry-end" v-model="end" required />
            </div>
          </template>
          <div v-else class="col-span-2 flex items-end pb-2 text-sm text-muted-foreground">{{ $t('timesheet.running') }}</div>
        </div>
        <div class="grid grid-cols-2 items-start gap-3">
          <div class="grid gap-2">
            <Label>{{ $t('entry.activity') }}</Label>
            <ActivitySelect v-model="activityId" :invalid="activityMissing" />
            <p v-if="activities.active.length === 0" class="text-xs text-muted-foreground">
              {{ $t('activities.noneYet') }}
              <RouterLink to="/activities" class="underline" @click="open = false">{{ $t('activities.manage') }}</RouterLink>
            </p>
          </div>
          <div class="grid gap-2">
            <Label>{{ $t('entry.project') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
            <ProjectSelect v-model="projectId" :include-archived="entry !== null" />
          </div>
        </div>
        <div class="grid grid-cols-4 items-end gap-3">
          <div class="col-span-2 grid gap-2">
            <Label>{{ $t('entry.tags') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
            <TagInput v-model="tagIds" :project-id="projectId" />
          </div>
          <div v-if="costCenters.all.length" class="grid gap-2">
            <Label>{{ $t('costCenters.costCenter') }}</Label>
            <CostCenterSelect :model-value="costCenterId" :empty-label="$t('costCenters.automatic')" @update:model-value="setCostCenter" />
          </div>
          <label class="flex h-9 items-center gap-2 text-sm">
            <Switch v-model="billable" />
            {{ $t('entry.billable') }}
          </label>
        </div>
        <div v-if="noteChoices.length > 1" class="grid gap-2">
          <Label>{{ $t('timesheet.bulk.noteChoose') }}</Label>
          <div class="grid max-h-32 gap-1 overflow-y-auto rounded-md border p-2">
            <label v-for="c in noteChoices" :key="c.id" class="flex items-start gap-2 text-sm">
              <Checkbox class="mt-0.5" :model-value="chosenNotes.includes(c.id)" @update:model-value="toggleNote(c.id, $event === true)" />
              <span class="shrink-0 text-xs text-muted-foreground tabular-nums">{{ c.when }}</span>
              <span class="line-clamp-2 min-w-0 whitespace-pre-line">{{ c.text }}</span>
            </label>
          </div>
          <p class="text-xs text-muted-foreground">{{ $t('timesheet.bulk.notePreview') }}</p>
        </div>
        <div class="flex min-h-32 flex-1 flex-col gap-2">
          <Label for="entry-note">{{ $t('entry.note') }}</Label>
          <NoteEditor id="entry-note" v-model="note" :placeholder="$t('entry.notePlaceholder')" fill min-height="4rem" />
        </div>
        <DialogFooter class="mt-auto">
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
  </form>
</template>
