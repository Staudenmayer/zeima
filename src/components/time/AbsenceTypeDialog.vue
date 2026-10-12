<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { commands, type AbsenceType } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { nextColor } from '@/lib/colors'
import { call, errorMessage } from '@/lib/ipc'
import { useWorkStore } from '@/stores/work'
import ColorPicker from './ColorPicker.vue'

const props = defineProps<{ type: AbsenceType | null }>()
const open = defineModel<boolean>('open', { required: true })

const work = useWorkStore()
const name = ref('')
const color = ref('')
const reducesTarget = ref(true)
const affectsVacation = ref(false)

watch(open, (isOpen) => {
  if (!isOpen) return
  const t = props.type
  name.value = t ? work.absenceTypeName(t) : ''
  color.value = t?.color ?? nextColor(work.absenceTypes.map((x) => x.color))
  reducesTarget.value = t?.reducesTarget ?? true
  affectsVacation.value = t?.affectsVacation ?? false
})

async function save() {
  try {
    await call(
      commands.absenceTypeSave(props.type?.id ?? null, {
        name: name.value,
        color: color.value,
        reducesTarget: reducesTarget.value,
        affectsVacation: affectsVacation.value,
      }),
    )
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ type ? $t('common.edit') : $t('absences.addType') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-2">
          <Label for="type-name">{{ $t('absences.typeName') }}</Label>
          <Input id="type-name" v-model="name" required :disabled="type?.key != null" />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('tags.color') }}</Label>
          <ColorPicker v-model="color" />
        </div>
        <div class="flex items-center justify-between gap-4 text-sm">
          <div>
            <div>{{ $t('absences.reducesTarget') }}</div>
            <p class="text-xs text-muted-foreground">{{ $t('absences.reducesTargetHint') }}</p>
          </div>
          <Switch v-model="reducesTarget" />
        </div>
        <label class="flex items-center justify-between gap-4 text-sm">
          {{ $t('absences.affectsVacation') }}
          <Switch v-model="affectsVacation" />
        </label>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
