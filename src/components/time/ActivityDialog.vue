<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import type { Activity, ActivityInput } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { nextColor } from '@/lib/colors'
import { errorMessage } from '@/lib/ipc'
import { useActivitiesStore } from '@/stores/activities'
import ColorPicker from './ColorPicker.vue'

const props = defineProps<{ activity: Activity | null }>()
const open = defineModel<boolean>('open', { required: true })

const activities = useActivitiesStore()
const form = ref<ActivityInput>({ name: '', color: '', archived: false })
const saving = ref(false)

watch(open, (isOpen) => {
  if (!isOpen) return
  form.value = props.activity
    ? { name: props.activity.name, color: props.activity.color, archived: props.activity.archived }
    : { name: '', color: nextColor(activities.all.map((a) => a.color)), archived: false }
})

async function save() {
  saving.value = true
  try {
    if (props.activity) await activities.update(props.activity.id, form.value)
    else await activities.create(form.value)
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
        <DialogTitle>{{ activity ? $t('activities.edit') : $t('activities.new') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-2">
          <Label for="activity-name">{{ $t('activities.name') }}</Label>
          <Input id="activity-name" v-model="form.name" required autofocus :placeholder="$t('activities.namePlaceholder')" />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('activities.color') }}</Label>
          <ColorPicker v-model="form.color" />
        </div>
        <div v-if="activity" class="flex items-center justify-between gap-2">
          <Label for="activity-archived">{{ $t('activities.archived') }}</Label>
          <Switch id="activity-archived" v-model="form.archived" />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
