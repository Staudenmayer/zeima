<script setup lang="ts">
import type { AddonTimelineItem } from '@/bindings'
import MeetingDetails from '@/components/time/MeetingDetails.vue'
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover'
import type { Anchor } from '@/lib/anchor'

// A meeting of the timesheet opened next to where it was clicked, like an
// entry: who takes part and what each answered, your own answer, and tracking it.
defineProps<{ item: AddonTimelineItem | null; reference: Anchor | null }>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ track: [item: AddonTimelineItem]; answered: [] }>()
</script>

<template>
  <Popover v-model:open="open">
    <PopoverAnchor :reference="reference ?? undefined" />
    <PopoverContent
      v-if="item"
      side="right"
      align="start"
      :side-offset="6"
      :collision-padding="8"
      class="max-h-(--reka-popover-content-available-height) w-80 overflow-x-hidden overflow-y-auto p-3"
      @open-auto-focus.prevent
    >
      <MeetingDetails :item="item" :active="open" heading @track="emit('track', $event)" @answered="emit('answered')" @close="open = false" />
    </PopoverContent>
  </Popover>
</template>
