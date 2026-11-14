<script setup lang="ts">
import { parseDate, type DateValue } from '@internationalized/date'
import { CalendarDays } from '@lucide/vue'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { getWeekStart } from '@/lib/time'
import { cn } from '@/lib/utils'

// A month chosen in the calendar (shadcn-vue): its heading browses the months,
// and a click on any day takes that day's month.
const model = defineModel<string>({ required: true }) // `YYYY-MM`
const props = defineProps<{ class?: string; ariaLabel?: string }>()
const { locale } = useI18n()
const open = ref(false)

const shown = computed<DateValue | undefined>(() => {
  try {
    return parseDate(`${model.value}-01`)
  } catch {
    return undefined
  }
})
const label = computed(() => (shown.value ? new Date(shown.value.year, shown.value.month - 1, 1).toLocaleDateString(locale.value, { month: 'long', year: 'numeric' }) : model.value))
function pick(value: DateValue | undefined) {
  if (!value) return
  model.value = value.toString().slice(0, 7)
  open.value = false
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <Button variant="outline" :class="cn('justify-between font-normal', props.class)" :aria-label="ariaLabel">
        {{ label }}
        <CalendarDays class="size-4 text-muted-foreground" />
      </Button>
    </PopoverTrigger>
    <PopoverContent align="start" class="w-auto p-0">
      <Calendar
        :default-placeholder="shown"
        :locale="locale"
        :week-starts-on="(getWeekStart() as 0 | 1 | 2 | 3 | 4 | 5 | 6)"
        initial-focus
        @update:model-value="pick"
      />
    </PopoverContent>
  </Popover>
</template>
