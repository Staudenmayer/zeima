<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'
import { computed, ref } from 'vue'
import type { Room } from '@/bindings'
import { Input } from '@/components/ui/input'

// The place of a meeting: one field that can stay empty, hold any text, or be a room of the
// organization (suggested as you type, with their seats and whether they are booked).
const props = defineProps<{ id?: string; rooms: Room[]; booked: Set<string> }>()
const roomId = defineModel<string | null>('roomId', { required: true })
const location = defineModel<string>('location', { required: true })

const open = ref(false)
const root = ref<HTMLElement | null>(null)
onClickOutside(root, () => (open.value = false))

const room = computed(() => props.rooms.find((r) => r.id === roomId.value) ?? null)
const text = computed(() => room.value?.name ?? location.value)
const options = computed(() => {
  const q = room.value ? '' : location.value.trim().toLowerCase()
  return props.rooms.filter((r) => !q || r.name.toLowerCase().includes(q))
})

function type(value: string | number) {
  roomId.value = null
  location.value = String(value)
  open.value = true
}
function pick(id: string) {
  roomId.value = id
  location.value = ''
  open.value = false
}
</script>

<template>
  <div ref="root" class="relative">
    <Input :id="id" :model-value="text" maxlength="200" autocomplete="off" role="combobox" :aria-expanded="open" :placeholder="$t('meetings.placeSearch')" @focus="open = true" @update:model-value="type" @keydown.esc="open = false" />
    <ul v-if="open && options.length" class="absolute top-full right-0 left-0 z-50 mt-1 max-h-56 overflow-y-auto rounded-md border bg-popover p-1 text-sm text-popover-foreground shadow-md">
      <li v-for="r in options" :key="r.id">
        <button type="button" class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-accent" @click="pick(r.id)">
          <span class="flex-1 truncate">{{ r.name }}</span>
          <span v-if="booked.has(r.id)" class="text-xs text-amber-600">{{ $t('meetings.booked') }}</span>
          <span v-else-if="r.capacity" class="text-xs text-muted-foreground">{{ $t('rooms.capacity', { n: r.capacity }) }}</span>
        </button>
      </li>
    </ul>
  </div>
</template>
