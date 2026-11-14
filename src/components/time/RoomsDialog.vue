<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Plus, Trash2 } from '@lucide/vue'
import { commands, type Room } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { call, errorMessage } from '@/lib/ipc'

// The rooms of the organization that meetings are held in: a name and, if it
// matters, how many people fit. A meeting names a room; a room is booked by
// one meeting at a time.
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ changed: [] }>()

const rooms = ref<Room[]>([])
const draft = ref({ name: '', capacity: '' })

async function load() {
  try {
    rooms.value = await call(commands.roomsList())
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
watch(open, (isOpen) => isOpen && void load(), { immediate: true })

async function run(action: () => Promise<unknown>) {
  try {
    await action()
    await load()
    emit('changed')
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const add = () =>
  run(async () => {
    await call(commands.roomSave(null, draft.value.name, draft.value.capacity ? Number(draft.value.capacity) : null))
    draft.value = { name: '', capacity: '' }
  })
const remove = (id: string) => run(() => call(commands.roomDelete(id)))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ $t('rooms.title') }}</DialogTitle>
        <DialogDescription>{{ $t('rooms.hint') }}</DialogDescription>
      </DialogHeader>
      <ul v-if="rooms.length" class="divide-y rounded-md border text-sm">
        <li v-for="r in rooms" :key="r.id" class="flex items-center gap-3 px-3 py-2">
          <span class="flex-1">{{ r.name }}</span>
          <span class="text-xs text-muted-foreground">{{ r.capacity ? $t('rooms.capacity', { n: r.capacity }) : $t('rooms.noLimit') }}</span>
          <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="remove(r.id)"><Trash2 /></Button>
        </li>
      </ul>
      <p v-else class="text-sm text-muted-foreground">{{ $t('rooms.none') }}</p>
      <form class="flex gap-2" @submit.prevent="add">
        <Input v-model="draft.name" :placeholder="$t('rooms.name')" required maxlength="100" />
        <Input v-model="draft.capacity" type="number" min="1" class="w-28" :placeholder="$t('rooms.capacityPlaceholder')" />
        <Button type="submit" :title="$t('rooms.add')"><Plus /></Button>
      </form>
    </DialogContent>
  </Dialog>
</template>
