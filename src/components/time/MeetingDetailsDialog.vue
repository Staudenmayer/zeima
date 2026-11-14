<script setup lang="ts">
import type { AddonTimelineItem, Meeting } from '@/bindings'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import MeetingDetails from '@/components/time/MeetingDetails.vue'

// A meeting opened in a dialog (the Meetings page).
withDefaults(defineProps<{ item: AddonTimelineItem | null; canTrack?: boolean; canEdit?: boolean }>(), { canTrack: true, canEdit: false })
const open = defineModel<boolean>('open', { required: true })
defineEmits<{ track: [item: AddonTimelineItem]; edit: [meeting: Meeting]; answered: [] }>()
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ item?.title }}</DialogTitle></DialogHeader>
      <MeetingDetails :item="item" :active="open" :can-track="canTrack" :can-edit="canEdit" @track="$emit('track', $event)" @edit="$emit('edit', $event)" @answered="$emit('answered')" @close="open = false" />
    </DialogContent>
  </Dialog>
</template>
