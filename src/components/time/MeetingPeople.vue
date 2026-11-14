<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { MeetingOccurrence } from '@/bindings'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { ANSWER_COLORS, participantsOf } from '@/lib/meetingPeople'
import { useOrgStore } from '@/stores/org'

// Who takes part in a meeting and what each answered, as a row of pictures:
// the ring is green for yes, amber for maybe, red for no, grey for no answer yet.
const props = withDefaults(defineProps<{ occurrence: MeetingOccurrence; max?: number }>(), { max: 5 })
const { t } = useI18n()
const org = useOrgStore()

const people = computed(() => participantsOf(props.occurrence))
const shown = computed(() => people.value.slice(0, props.max))
const more = computed(() => people.value.length - shown.value.length)
const label = (id: string, answer: string) => `${org.nameOf(id) || t('team.someone')} · ${t(`meetings.answers.${answer}`)}`
</script>

<template>
  <div class="flex items-center">
    <span
      v-for="p in shown"
      :key="p.userId"
      class="-ml-1 inline-flex rounded-full first:ml-0"
      :style="{ boxShadow: `0 0 0 2px ${ANSWER_COLORS[p.answer]}` }"
      :title="label(p.userId, p.answer)"
    >
      <UserAvatar :user-id="p.userId" size="sm" />
    </span>
    <span v-if="more > 0" class="ml-1.5 text-xs text-muted-foreground" :title="people.slice(max).map((p) => label(p.userId, p.answer)).join('\n')">+{{ more }}</span>
  </div>
</template>
