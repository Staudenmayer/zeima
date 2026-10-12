<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands, type EntryInput, type TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import TimeInput from '@/components/time/TimeInput.vue'
import DateInput from '@/components/time/DateInput.vue'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { call, errorMessage } from '@/lib/ipc'
import { HOUR, fromInputs, rangeFromInputs, toDateInput, toTimeInput } from '@/lib/time'
import { useCostCentersStore } from '@/stores/costCenters'
import { useProjectsStore } from '@/stores/projects'
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

const { t } = useI18n()
const projects = useProjectsStore()
const costCenters = useCostCentersStore()

const projectId = ref<string | null>(null)
const tagIds = ref<string[]>([])
const date = ref('')
const start = ref('')
const end = ref('')
const note = ref('')
const billable = ref(false)
/** `null` = automatic (project default, then the user's default). */
const costCenterId = ref<string | null>(null)
/** Picked by the user; otherwise it is resolved again on save. */
const costCenterTouched = ref(false)
const saving = ref(false)

const running = computed(() => props.entry !== null && props.entry.endAt === null)

watch(open, (isOpen) => {
  if (!isOpen) return
  const e = props.entry
  if (e) {
    projectId.value = e.projectId
    tagIds.value = [...e.tagIds]
    date.value = toDateInput(e.startAt)
    start.value = toTimeInput(e.startAt)
    end.value = e.endAt === null ? '' : toTimeInput(e.endAt)
    note.value = e.note
    billable.value = e.billable
    costCenterId.value = e.costCenterId
  } else {
    // New entry: the hour before now, or 09:00–10:00 on another day.
    const now = Date.now()
    const day = props.day ?? now
    const isToday = toDateInput(day) === toDateInput(now)
    const startAt = isToday ? now - HOUR : fromInputs(toDateInput(day), '09:00')
    projectId.value = null
    tagIds.value = []
    date.value = toDateInput(day)
    start.value = toTimeInput(startAt)
    end.value = toTimeInput(startAt + HOUR)
    note.value = ''
    billable.value = false
    costCenterId.value = null
  }
  costCenterTouched.value = false
})

// New entries take the project's billable default.
watch(projectId, (id) => {
  if (props.entry) return
  billable.value = id ? (projects.byId.get(id)?.billableDefault ?? false) : false
})

async function save() {
  let startAt: number
  let endAt: number | null
  if (running.value) {
    startAt = fromInputs(date.value, start.value)
    endAt = null
  } else {
    ;({ startAt, endAt } = rangeFromInputs(date.value, start.value, end.value))
  }
  if (Number.isNaN(startAt) || (endAt !== null && Number.isNaN(endAt))) {
    toast.error(t('entry.invalidTime'))
    return
  }
  const input: EntryInput = {
    projectId: projectId.value,
    startAt,
    endAt,
    tz: null,
    note: note.value,
    billable: billable.value,
    tagIds: tagIds.value,
    // Untouched: keep the entry's cost center unless the project changed.
    costCenterId: costCenterTouched.value
      ? costCenterId.value
      : props.entry && props.entry.projectId === projectId.value
        ? props.entry.costCenterId
        : null,
  }
  saving.value = true
  try {
    if (props.entry) await call(commands.entryUpdate(props.entry.id, input))
    else await call(commands.entryCreate(input))
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
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
          <Label>{{ $t('entry.project') }}</Label>
          <ProjectSelect v-model="projectId" :include-archived="entry !== null" />
        </div>
        <div class="grid grid-cols-3 gap-3">
          <div class="grid gap-2">
            <Label for="entry-date">{{ $t('entry.date') }}</Label>
            <DateInput id="entry-date" v-model="date" required />
          </div>
          <div class="grid gap-2">
            <Label for="entry-start">{{ $t('entry.start') }}</Label>
            <TimeInput id="entry-start" v-model="start" required />
          </div>
          <div class="grid gap-2">
            <Label for="entry-end">{{ $t('entry.end') }}</Label>
            <TimeInput v-if="!running" id="entry-end" v-model="end" required />
            <div v-else class="flex h-9 items-center text-sm text-muted-foreground">{{ $t('timesheet.running') }}</div>
          </div>
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('entry.tags') }}</Label>
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
            @update:model-value="costCenterId = $event; costCenterTouched = true"
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
