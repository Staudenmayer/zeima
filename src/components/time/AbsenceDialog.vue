<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { commands, type Absence, type HalfDay } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import DateInput from '@/components/time/DateInput.vue'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { call, errorMessage } from '@/lib/ipc'
import { toDateInput } from '@/lib/time'
import { useWorkStore } from '@/stores/work'

const props = defineProps<{ absence: Absence | null }>()
const open = defineModel<boolean>('open', { required: true })

const work = useWorkStore()
const typeId = ref('')
const startDate = ref('')
const endDate = ref('')
const half = ref<'none' | HalfDay>('none')
const note = ref('')
const saving = ref(false)

const singleDay = computed(() => startDate.value === endDate.value)

watch(open, (isOpen) => {
  if (!isOpen) return
  const a = props.absence
  const today = toDateInput(Date.now())
  typeId.value = a?.typeId ?? work.absenceTypes[0]?.id ?? ''
  startDate.value = a?.startDate ?? today
  endDate.value = a?.endDate ?? today
  half.value = a?.halfDay ?? 'none'
  note.value = a?.note ?? ''
})

watch(startDate, (start) => {
  if (endDate.value < start) endDate.value = start
})
watch(singleDay, (single) => {
  if (!single) half.value = 'none'
})

async function save() {
  saving.value = true
  try {
    await call(
      commands.absenceSave(props.absence?.id ?? null, {
        typeId: typeId.value,
        startDate: startDate.value,
        endDate: endDate.value,
        halfDay: half.value === 'none' ? null : half.value,
        note: note.value,
      }),
    )
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
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ absence ? $t('absences.edit') : $t('absences.add') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-2">
          <Label>{{ $t('absences.type') }}</Label>
          <Select v-model="typeId">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="t in work.absenceTypes" :key="t.id" :value="t.id">
                <span class="inline-flex items-center gap-2">
                  <span class="size-2.5 rounded-full" :style="{ backgroundColor: t.color }" />
                  {{ work.absenceTypeName(t) }}
                </span>
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2">
            <Label for="absence-from">{{ $t('absences.from') }}</Label>
            <DateInput id="absence-from" v-model="startDate" required />
          </div>
          <div class="grid gap-2">
            <Label for="absence-to">{{ $t('absences.to') }}</Label>
            <DateInput id="absence-to" v-model="endDate" :min="startDate" required />
          </div>
        </div>
        <div v-if="singleDay" class="grid gap-2">
          <Label>{{ $t('absences.halfDay') }}</Label>
          <Select v-model="half">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="h in ['none', 'am', 'pm']" :key="h" :value="h">{{ $t(`absences.halfDays.${h}`) }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2">
          <Label for="absence-note">{{ $t('entry.note') }}</Label>
          <Textarea id="absence-note" v-model="note" rows="2" />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving || !typeId">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
