<script setup lang="ts">
import { ExternalLink, MapPin, Pencil, Play } from '@lucide/vue'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import { commands, type AddonTimelineItem, type Meeting, type MeetingOccurrence, type RsvpStatus } from '@/bindings'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { Button } from '@/components/ui/button'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { ANSWER_COLORS, participantsOf } from '@/lib/meetingPeople'
import { toDateInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'

// A meeting opened: when and where, who takes part and what each answered
// (a name opens the coworkers list; the person can change their own answer);
// "Track this event" starts the entry the timesheet offers for a meeting.
// Shown in a popover next to the click (timesheet) or in a dialog (Meetings).
const props = withDefaults(defineProps<{ item: AddonTimelineItem | null; active: boolean; heading?: boolean; canTrack?: boolean; canEdit?: boolean }>(), { heading: false, canTrack: true, canEdit: false })
const emit = defineEmits<{ track: [item: AddonTimelineItem]; edit: [meeting: Meeting]; answered: []; close: [] }>()
const format = useFormat()
const org = useOrgStore()
const account = useAccountStore()
const router = useRouter()
const projects = useProjectsStore()

const occurrence = ref<MeetingOccurrence | null>(null)
async function load() {
  const item = props.item
  if (!props.active || !item) return
  const meetingId = item.id.split('@')[0]
  const day = toDateInput(item.startAt)
  try {
    occurrence.value = (await call(commands.meetingsList(day, day))).find((o) => o.meeting.id === meetingId && o.startAt === item.startAt) ?? null
  } catch {
    occurrence.value = null
  }
}
// An answer can be for all dates of a repeating meeting or for this date only.
const dateOnly = ref(false)
watch(
  () => [props.active, props.item] as const,
  async () => {
    occurrence.value = null
    await load()
    const loaded = occurrence.value as MeetingOccurrence | null
    dateOnly.value = loaded?.myDateAnswer ?? false
  },
  { immediate: true },
)

/** The person is asked to this meeting (they are not its organizer) and it is on. */
const mayAnswer = computed(() => !!occurrence.value && !occurrence.value.meeting.cancelled && occurrence.value.meeting.attendeeIds.includes(account.status.user?.id ?? ''))
const repeating = computed(() => !!occurrence.value?.meeting.rrule)
const ANSWERS: { status: RsvpStatus; label: string }[] = [
  { status: 'accepted', label: 'meetings.accept' },
  { status: 'tentative', label: 'meetings.maybe' },
  { status: 'declined', label: 'meetings.decline' },
]
async function answer(status: RsvpStatus) {
  const o = occurrence.value
  if (!o) return
  try {
    await call(commands.meetingRespond(o.meeting.id, status, repeating.value && dateOnly.value ? o.startAt : null))
    await load()
    emit('answered')
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function chooseScope(only: boolean) {
  const o = occurrence.value
  dateOnly.value = only
  // Back to all dates: the answer for this date is taken back, so the series answer counts again.
  if (!only && o?.myDateAnswer) {
    try {
      await call(commands.meetingRespondClear(o.meeting.id, o.startAt))
      await load()
      emit('answered')
    } catch (err) {
      toast.error(errorMessage(err))
    }
  }
}
/** A name opens the coworkers list for that person. */
function showPerson(userId: string) {
  emit('close')
  void router.push({ path: '/directory', query: { user: userId } })
}
</script>

<template>
  <div class="grid gap-3">
    <div v-if="item" class="grid min-w-0 gap-3 text-sm">
    <h2 v-if="heading" class="truncate text-base font-semibold">{{ item.title }}</h2>
        <p class="tabular-nums text-muted-foreground">{{ format.day(item.startAt) }} · {{ format.time(item.startAt) }}–{{ format.time(item.endAt) }}</p>
        <template v-if="occurrence">
          <p v-if="occurrence.meeting.location" class="flex items-center gap-1.5"><MapPin class="size-4 shrink-0 text-muted-foreground" /> {{ occurrence.meeting.location }}</p>
          <button v-if="occurrence.meeting.onlineUrl" type="button" class="flex items-center gap-1.5 hover:underline" @click="openUrl(occurrence.meeting.onlineUrl)"><ExternalLink class="size-4 shrink-0 text-muted-foreground" /> {{ $t('meetings.join') }}</button>
          <p v-if="occurrence.meeting.projectId && projects.byId.get(occurrence.meeting.projectId)" class="text-muted-foreground">{{ projects.byId.get(occurrence.meeting.projectId)?.name }}</p>
          <p v-if="occurrence.meeting.note" class="text-muted-foreground">{{ occurrence.meeting.note }}</p>
          <div class="grid gap-1.5">
            <h3 class="text-xs font-medium text-muted-foreground">{{ $t('meetings.participants') }}</h3>
            <ul class="grid gap-1.5">
              <li v-for="p in participantsOf(occurrence)" :key="p.userId" class="flex items-center gap-2">
                <button type="button" class="flex min-w-0 flex-1 items-center gap-2 text-left hover:underline" :title="$t('directory.open')" @click="showPerson(p.userId)">
                  <UserAvatar :user-id="p.userId" size="sm" />
                  <span class="truncate">{{ org.nameOf(p.userId) || $t('team.someone') }}</span>
                </button>
                <span class="flex items-center gap-1.5 text-xs" :style="{ color: ANSWER_COLORS[p.answer] }">
                  <span class="size-2 rounded-full" :style="{ backgroundColor: ANSWER_COLORS[p.answer] }" />{{ $t(`meetings.answers.${p.answer}`) }}
                </span>
              </li>
            </ul>
          </div>
          <div v-if="mayAnswer" class="grid gap-1.5">
            <h3 class="text-xs font-medium text-muted-foreground">{{ $t('meetings.answer') }}</h3>
            <div class="flex flex-wrap items-center gap-1">
              <Button v-for="a in ANSWERS" :key="a.status" size="sm" class="h-7" :variant="occurrence.myStatus === a.status ? (a.status === 'declined' ? 'destructive' : 'default') : 'outline'" @click="answer(a.status)">{{ $t(a.label) }}</Button>
            </div>
            <div v-if="repeating" class="flex items-center gap-1 text-xs">
              <button type="button" class="rounded px-1.5 py-0.5" :class="!dateOnly ? 'bg-muted font-medium' : 'text-muted-foreground hover:bg-muted/60'" @click="chooseScope(false)">{{ $t('meetings.allDates') }}</button>
              <button type="button" class="rounded px-1.5 py-0.5" :class="dateOnly ? 'bg-muted font-medium' : 'text-muted-foreground hover:bg-muted/60'" @click="chooseScope(true)">{{ $t('meetings.thisDate') }}</button>
            </div>
          </div>
        </template>
        <p v-else class="text-muted-foreground">{{ item.detail }}</p>
      </div>
      <div class="flex flex-wrap justify-end gap-2">
        <Button type="button" variant="outline" @click="emit('close')">{{ $t('common.close') }}</Button>
        <Button v-if="canEdit && occurrence" type="button" variant="outline" @click="(emit('close'), emit('edit', occurrence.meeting))"><Pencil /> {{ $t('common.edit') }}</Button>
        <Button v-if="item && canTrack" type="button" @click="(emit('close'), emit('track', item))"><Play /> {{ $t('timesheet.trackEvent') }}</Button>
      </div>
  </div>
</template>
