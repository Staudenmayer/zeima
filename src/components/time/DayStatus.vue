<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { DayWarning, WorkDay } from '@/bindings'
import { useSettingsStore } from '@/stores/settings'
import { useWorkStore } from '@/stores/work'

/** Target, difference, holiday, absences and warnings of one day. */
defineProps<{ day: WorkDay; holidayName?: string }>()

const { t } = useI18n()
const settings = useSettingsStore()
const work = useWorkStore()

function warning(w: DayWarning): string {
  switch (w.kind) {
    case 'exceedsDailyMax':
      return t('work.warnings.exceedsDailyMax', { h: settings.formatNumber(w.maxMinutes / 60, 1) })
    case 'restTooShort':
      return t('work.warnings.restTooShort', { rest: settings.formatNumber(w.restMinutes / 60, 1), min: settings.formatNumber(w.minMinutes / 60, 1) })
    case 'breakTooShort':
      return t('work.warnings.breakTooShort', { taken: w.takenMinutes, required: w.requiredMinutes })
  }
}
</script>

<template>
  <span class="inline-flex flex-wrap items-center gap-1.5 text-xs">
    <span v-if="day.holiday" class="rounded-full bg-purple-500/15 px-2 py-0.5 text-purple-700 dark:text-purple-300">
      {{ holidayName ?? $t('work.holiday') }}
    </span>
    <span
      v-for="id in day.absenceTypeIds"
      :key="id"
      class="rounded-full px-2 py-0.5 text-white"
      :style="{ backgroundColor: work.absenceTypesById.get(id)?.color }"
    >{{ work.absenceTypesById.get(id) ? work.absenceTypeName(work.absenceTypesById.get(id)!) : '' }}</span>
    <span v-for="(w, i) in day.warnings" :key="i" class="rounded-full bg-destructive/15 px-2 py-0.5 text-destructive">
      ⚠ {{ warning(w) }}
    </span>
    <span v-if="day.breakDeductedMs > 0" class="text-muted-foreground">
      {{ $t('work.breakDeducted', { min: Math.round(day.breakDeductedMs / 60000) }) }}
    </span>
    <span v-if="day.targetMs > 0 || day.netMs > 0" class="text-muted-foreground">
      {{ settings.formatDuration(day.netMs) }} / {{ settings.formatDuration(day.targetMs) }} h
    </span>
    <span
      v-if="day.deltaMs !== 0"
      class="font-medium tabular-nums"
      :class="day.deltaMs > 0 ? 'text-green-600 dark:text-green-400' : 'text-amber-600 dark:text-amber-400'"
    >{{ settings.formatSigned(day.deltaMs) }}</span>
  </span>
</template>
