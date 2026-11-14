<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { PRESENCE_COLORS } from '@/lib/presence'
import { useOrgStore } from '@/stores/org'

// Whether a person is online: a coloured dot with the state as its tooltip
// (green online, red in a meeting, purple traveling, amber away),
// grey when they are offline. Presence is what the
// organization's devices report (see Settings for what people share).
const props = defineProps<{ userId: string }>()
const { t } = useI18n()
const org = useOrgStore()

const state = computed(() => org.presence.find((p) => p.userId === props.userId)?.state ?? null)
const label = computed(() => (state.value ? t(`org.presenceStates.${state.value}`) : t('org.offline')))
</script>

<template>
  <span
    class="inline-block aspect-square size-2.5 min-h-2.5 min-w-2.5 shrink-0 grow-0 rounded-full"
    :style="{ backgroundColor: PRESENCE_COLORS[state ?? 'offline'] }"
    :title="label"
    role="img"
    :aria-label="label"
  />
</template>
