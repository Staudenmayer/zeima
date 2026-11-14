<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useOrgStore } from '@/stores/org'

// Whether a person is online: a green/blue/amber/grey dot with the state as
// its tooltip, or an empty ring when they are not. Presence is what the
// organization's devices report (see Settings for what people share).
const props = defineProps<{ userId: string }>()
const { t } = useI18n()
const org = useOrgStore()

const COLORS: Record<string, string> = { online: '#22c55e', tracking: '#3b82f6', in_meeting: '#f59e0b', away: '#9ca3af' }
const state = computed(() => org.presence.find((p) => p.userId === props.userId)?.state ?? null)
const label = computed(() => (state.value ? t(`org.presenceStates.${state.value}`) : t('org.offline')))
</script>

<template>
  <span
    class="inline-block size-2.5 shrink-0 rounded-full"
    :class="state ? '' : 'border border-muted-foreground/50'"
    :style="state ? { backgroundColor: COLORS[state] } : undefined"
    :title="label"
    role="img"
    :aria-label="label"
  />
</template>
