<script setup lang="ts">
import { watch } from 'vue'
import type { TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import TimeInput from '@/components/time/TimeInput.vue'
import DateInput from '@/components/time/DateInput.vue'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { type EntryDraft, useEntryForm } from '@/composables/useEntryForm'
import { useActivitiesStore } from '@/stores/activities'
import { useCostCentersStore } from '@/stores/costCenters'
import { useWorkspaceStore } from '@/stores/workspace'
import { fromInputs } from '@/lib/time'
import ActivitySelect from './ActivitySelect.vue'
import CostCenterSelect from './CostCenterSelect.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagPicker from './TagPicker.vue'

const props = defineProps<{
  /** Entry to edit; `null` creates a new one. */
  entry: TimeEntry | null
  /** Day preselected for new entries (unix ms). */
  day?: number
  /** Values for a new entry, e.g. from a calendar event. */
  draft?: EntryDraft | null
}>()
const emit = defineEmits<{ /** "Meeting" was chosen: the times so far, for the meeting dialog. */ meeting: [times: { startAt: number; endAt: number }] }>()
const open = defineModel<boolean>('open', { required: true })
const workspaces = useWorkspaceStore()

const activities = useActivitiesStore()
const costCenters = useCostCentersStore()

const { activityId, activityMissing, projectId, tagIds, date, endDate, start, end, note, billable, costCenterId, saving, running, reset, setCostCenter, save: saveForm } =
  useEntryForm(() => props.entry, () => props.day, () => props.draft ?? null)

watch(open, (isOpen) => {
  if (isOpen) reset()
})

/** In an organization a new item can be a meeting too: the switch hands the times over to the meeting dialog. */
function toMeeting() {
  const startAt = fromInputs(date.value, start.value)
  let endAt = fromInputs(date.value, end.value)
  if (Number.isNaN(endAt) || endAt <= startAt) endAt = startAt + 3_600_000
  open.value = false
  emit('meeting', { startAt: Number.isNaN(startAt) ? Date.now() : startAt, endAt })
}

async function save() {
  if (await saveForm()) open.value = false
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="h-[min(40rem,94vh)] grid-rows-[auto_1fr] overflow-y-auto sm:max-w-5xl">
      <DialogHeader>
        <DialogTitle>{{ entry ? $t('timesheet.editEntry') : $t('timesheet.addEntry') }}</DialogTitle>
      </DialogHeader>
      <form class="flex flex-col gap-4" @submit.prevent="save">
        <div v-if="!entry && workspaces.inOrg" role="group" :aria-label="$t('timesheet.whatToAdd')" class="inline-flex gap-0.5 self-start justify-self-start rounded-lg bg-muted p-0.5">
          <Button type="button" size="sm" variant="outline" class="h-7" aria-pressed="true">{{ $t('timesheet.addKindEntry') }}</Button>
          <Button type="button" size="sm" variant="ghost" class="h-7" aria-pressed="false" @click="toMeeting">{{ $t('timesheet.addKindMeeting') }}</Button>
        </div>
        <!-- Fields in rows of two so both columns line up: activity / start, project / end, tags / note, cost center / billable. -->
        <div class="grid grid-cols-2 items-start gap-x-8 gap-y-4">
          <div class="grid gap-2">
            <Label>{{ $t('entry.activity') }}</Label>
            <ActivitySelect v-model="activityId" :invalid="activityMissing" />
            <p v-if="activities.active.length === 0" class="text-xs text-muted-foreground">
              {{ $t('activities.noneYet') }}
              <RouterLink to="/activities" class="underline" @click="open = false">{{ $t('activities.manage') }}</RouterLink>
            </p>
          </div>
          <!-- Start and end each have a date: entries may span several days. -->
          <div class="grid gap-2">
            <Label for="entry-date">{{ $t('entry.start') }}</Label>
            <div class="grid grid-cols-[1fr_8rem] gap-2">
              <DateInput id="entry-date" v-model="date" required />
              <TimeInput id="entry-start" v-model="start" required :aria-label="$t('entry.start')" />
            </div>
          </div>
          <div class="grid gap-2">
            <Label>{{ $t('entry.project') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
            <ProjectSelect v-model="projectId" :include-archived="entry !== null" />
          </div>
          <div class="grid gap-2">
            <Label for="entry-end-date">{{ $t('entry.end') }}</Label>
            <div v-if="!running" class="grid grid-cols-[1fr_8rem] gap-2">
              <DateInput id="entry-end-date" v-model="endDate" required />
              <TimeInput id="entry-end" v-model="end" required :aria-label="$t('entry.end')" />
            </div>
            <div v-else class="flex h-9 items-center text-sm text-muted-foreground">{{ $t('timesheet.running') }}</div>
          </div>
          <div class="grid gap-2">
            <Label>{{ $t('entry.tags') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
            <TagPicker v-model="tagIds" :project-id="projectId" />
          </div>
          <div class="grid gap-2">
            <Label for="entry-note">{{ $t('entry.note') }}</Label>
            <Textarea id="entry-note" v-model="note" :placeholder="$t('entry.notePlaceholder')" rows="4" />
          </div>
          <div v-if="costCenters.all.length" class="grid gap-2">
            <Label>{{ $t('costCenters.costCenter') }}</Label>
            <CostCenterSelect
              :model-value="costCenterId"
              :empty-label="$t('costCenters.automatic')"
              @update:model-value="setCostCenter"
            />
          </div>
          <label class="flex items-center gap-2 text-sm" :class="costCenters.all.length ? 'h-9 self-end' : ''">
            <Switch v-model="billable" />
            {{ $t('entry.billable') }}
          </label>
        </div>
        <DialogFooter class="mt-auto">
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
