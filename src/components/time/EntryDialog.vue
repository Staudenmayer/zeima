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
import { useEntryForm } from '@/composables/useEntryForm'
import { useActivitiesStore } from '@/stores/activities'
import { useCostCentersStore } from '@/stores/costCenters'
import ActivitySelect from './ActivitySelect.vue'
import CostCenterSelect from './CostCenterSelect.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagPicker from './TagPicker.vue'

const props = defineProps<{
  /** Entry to edit; `null` creates a new one. */
  entry: TimeEntry | null
  /** Day preselected for new entries (unix ms). */
  day?: number
}>()
const open = defineModel<boolean>('open', { required: true })

const activities = useActivitiesStore()
const costCenters = useCostCentersStore()

const { activityId, activityMissing, projectId, tagIds, date, endDate, start, end, note, billable, costCenterId, saving, running, reset, setCostCenter, save: saveForm } =
  useEntryForm(() => props.entry, () => props.day)

watch(open, (isOpen) => {
  if (isOpen) reset()
})

async function save() {
  if (await saveForm()) open.value = false
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ entry ? $t('timesheet.editEntry') : $t('timesheet.addEntry') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
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
        <!-- Start and end each have a date: entries may span several days. -->
        <div class="grid grid-cols-[auto_1fr_8rem] items-center gap-x-3 gap-y-2">
          <Label for="entry-date">{{ $t('entry.start') }}</Label>
          <DateInput id="entry-date" v-model="date" required />
          <TimeInput id="entry-start" v-model="start" required :aria-label="$t('entry.start')" />
          <Label for="entry-end-date">{{ $t('entry.end') }}</Label>
          <template v-if="!running">
            <DateInput id="entry-end-date" v-model="endDate" required />
            <TimeInput id="entry-end" v-model="end" required :aria-label="$t('entry.end')" />
          </template>
          <div v-else class="col-span-2 flex h-9 items-center text-sm text-muted-foreground">{{ $t('timesheet.running') }}</div>
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('entry.tags') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
          <TagPicker v-model="tagIds" :project-id="projectId" />
        </div>
        <div class="grid gap-2">
          <Label for="entry-note">{{ $t('entry.note') }}</Label>
          <Textarea id="entry-note" v-model="note" :placeholder="$t('entry.notePlaceholder')" rows="2" />
        </div>
        <div v-if="costCenters.all.length" class="grid gap-2">
          <Label>{{ $t('costCenters.costCenter') }}</Label>
          <CostCenterSelect
            :model-value="costCenterId"
            :empty-label="$t('costCenters.automatic')"
            @update:model-value="setCostCenter"
          />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <Switch v-model="billable" />
          {{ $t('entry.billable') }}
        </label>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
