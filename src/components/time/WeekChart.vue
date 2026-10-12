<script setup lang="ts">
import type { ChartConfig } from '@/components/ui/chart'
import { VisAxis, VisGroupedBar, VisXYContainer } from '@unovis/vue'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSettingsStore } from '@/stores/settings'
import { ChartContainer, ChartCrosshair, ChartTooltip, ChartTooltipContent, componentToString } from '@/components/ui/chart'

const props = defineProps<{
  days: { label: string; hours: number; target: number }[]
}>()

const { t } = useI18n()
const settings = useSettingsStore()

type Datum = { index: number; label: string; hours: number; target: number }
const data = computed<Datum[]>(() => props.days.map((d, index) => ({ index, ...d })))

const config = computed(() => ({
  hours: { label: t('dashboard.hours'), color: 'var(--chart-2)' },
  target: { label: t('dashboard.target'), color: 'var(--muted-foreground)' },
}) satisfies ChartConfig)

const y = [(d: Datum) => d.hours, (d: Datum) => d.target]
const colors = computed(() => [config.value.hours.color, config.value.target.color])
</script>

<template>
  <ChartContainer :config="config" class="aspect-auto h-[220px] w-full" cursor>
    <VisXYContainer :data="data" :margin="{ left: -24 }" :y-domain="[0, undefined]">
      <VisGroupedBar
        :x="(d: Datum) => d.index"
        :y="y"
        :color="colors"
        :group-padding="0.3"
        :bar-padding="0.15"
        :rounded-corners="3"
      />
      <VisAxis
        type="x"
        :x="(d: Datum) => d.index"
        :tick-line="false"
        :domain-line="false"
        :grid-line="false"
        :num-ticks="data.length"
        :tick-format="(i: number) => data[i]?.label ?? ''"
      />
      <VisAxis type="y" :num-ticks="3" :tick-line="false" :domain-line="false" />
      <ChartTooltip />
      <ChartCrosshair
        :x="(d: Datum) => d.index"
        :y="y"
        :template="componentToString(config, ChartTooltipContent, {
          labelFormatter: (i: number | Date) => data[Number(i)]?.label ?? '',
          valueFormatter: (h: number) => `${settings.formatNumber(h)} h`,
        })"
        color="#0000"
      />
    </VisXYContainer>
  </ChartContainer>
</template>
