<script setup lang="ts">
import { computed, ref } from 'vue'
import { Play, X } from '@lucide/vue'
import type { AddonTimelineItem, Meeting, TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { EntryDraft } from '@/composables/useEntryForm'
import { useWorkspaceStore } from '@/stores/workspace'
import EntryForm from './EntryForm.vue'
import MeetingForm from './MeetingForm.vue'

// One dialog for time entries and meetings: the switch slides from one form to the other,
// the dialog itself stays open.
const props = defineProps<{
  entry: TimeEntry | null
  day?: number
  draft?: EntryDraft | null
  /** Combine these entries into one: the entry form, prefilled by `draft`. */
  merge?: TimeEntry[] | null
  meeting: Meeting | null
  meetingDraft?: { startAt: number; endAt: number } | null
  /** The meeting as a timeline item: the dialog then offers to track it. */
  trackItem?: AddonTimelineItem | null
}>()
const emit = defineEmits<{
  saved: []
  entrySaved: []
  track: [item: AddonTimelineItem]
  /** "Meeting" was chosen on a new item: the times so far. */
  toMeeting: [times: { startAt: number; endAt: number }]
  /** "Time entry" was chosen on a new meeting. */
  toEntry: [times: { startAt: number; endAt: number }]
}>()
const entryOpen = defineModel<boolean>('entryOpen', { required: true })
const meetingOpen = defineModel<boolean>('meetingOpen', { required: true })

const workspaces = useWorkspaceStore()
const isMeeting = computed(() => meetingOpen.value)
const open = computed(() => entryOpen.value || meetingOpen.value)
/** A new item can be either; an existing one stays what it is. */
const canSwitch = (entry: TimeEntry | null, meeting: Meeting | null) => !entry && !meeting && !props.merge && workspaces.inOrg

/** A new item can be either, so the switch is shown; an existing one stays what it is. */
const lead = computed(() => canSwitch(props.entry, props.meeting))

const entryForm = ref<InstanceType<typeof EntryForm> | null>(null)
const meetingForm = ref<InstanceType<typeof MeetingForm> | null>(null)

/** The pane that is not shown stays visible until the slide is over, then drops out of the tab order. */
/** Going back waits a moment, so the divider is gone before the slide starts. */
const BACK_DELAY = 0.12
const paneStyle = (hidden: boolean, wasMeeting: boolean) =>
  ({ visibility: hidden ? 'hidden' : 'visible', transition: hidden ? `visibility 0s linear ${0.3 + (wasMeeting ? BACK_DELAY : 0)}s` : 'visibility 0s' }) as const

function close(value: boolean) {
  if (!value) {
    entryOpen.value = false
    meetingOpen.value = false
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="close">
    <DialogContent :aria-describedby="undefined" :show-close-button="false" class="flex flex-col overflow-hidden sm:max-w-6xl" style="height: min(50rem, 97vh); max-height: min(50rem, 97vh)">
      <!-- The line before the attendees runs the whole height of the dialog; it fades in once the slide to the meeting form is over, and out as soon as the slide back starts. -->
      <div class="pointer-events-none absolute inset-y-0 right-[19.5rem] w-px bg-border" :style="{ opacity: isMeeting ? 1 : 0, transition: isMeeting ? 'opacity 0.2s ease 0.3s' : 'opacity 0.1s ease' }" aria-hidden="true" />
      <DialogHeader class="flex-row items-center gap-1">
        <DialogTitle class="min-w-0 flex-1 truncate">
          <template v-if="isMeeting">{{ meeting ? $t('meetings.edit') : $t('meetings.new') }}</template>
          <template v-else-if="merge">{{ $t('timesheet.bulk.mergeTitle', merge.length) }}</template>
          <template v-else>{{ entry ? $t('timesheet.editEntry') : $t('timesheet.addEntry') }}</template>
        </DialogTitle>
        <Button v-if="isMeeting && meeting && trackItem" type="button" size="icon" variant="ghost" class="size-7" :title="$t('timesheet.trackEvent')" :aria-label="$t('timesheet.trackEvent')" @click="(close(false), emit('track', trackItem))"><Play /></Button>
        <Button type="button" size="icon" variant="ghost" class="size-7" :title="$t('common.close')" :aria-label="$t('common.close')" @click="close(false)"><X /></Button>
      </DialogHeader>
      <!-- The forms start at the top of this area; on a new item the switch (with its label) floats over the
           room they leave free, so the people column of a meeting can start level with the switch. -->
      <div class="relative min-h-0 flex-1">
        <div v-if="lead" class="absolute top-0 left-0 z-10 grid gap-2 pl-0.5">
          <Label class="leading-none">{{ $t('timesheet.addKind') }}</Label>
          <div role="group" :aria-label="$t('timesheet.whatToAdd')" class="inline-flex gap-0.5 self-start rounded-lg bg-muted p-0.5">
            <Button type="button" size="sm" :variant="isMeeting ? 'ghost' : 'outline'" class="h-7" :aria-pressed="!isMeeting" @click="isMeeting && meetingForm?.toEntry()">{{ $t('timesheet.addKindEntry') }}</Button>
            <Button type="button" size="sm" :variant="isMeeting ? 'outline' : 'ghost'" class="h-7" :aria-pressed="isMeeting" @click="!isMeeting && entryForm?.toMeeting()">{{ $t('timesheet.addKindMeeting') }}</Button>
          </div>
        </div>
        <!-- Two panes side by side; the track slides to show one. -->
        <div class="h-full overflow-hidden">
          <div class="flex h-full w-[200%]" :style="{ transform: isMeeting ? 'translateX(-50%)' : 'translateX(0)', transition: `transform 0.3s ease-in-out ${isMeeting ? 0 : BACK_DELAY}s` }">
            <div class="h-full w-1/2 shrink-0" :aria-hidden="isMeeting" :style="paneStyle(isMeeting, false)">
              <EntryForm ref="entryForm" v-model:open="entryOpen" :entry="entry" :day="day" :draft="draft" :merge="merge" :lead="lead" @saved="emit('entrySaved')" @meeting="emit('toMeeting', $event)" />
            </div>
            <div class="h-full w-1/2 shrink-0" :aria-hidden="!isMeeting" :style="paneStyle(!isMeeting, true)">
              <MeetingForm ref="meetingForm" v-model:open="meetingOpen" :meeting="meeting" :draft="meetingDraft" :lead="lead" @saved="emit('saved')" @entry="emit('toEntry', $event)" />
            </div>
          </div>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
