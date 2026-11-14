<script setup lang="ts">
import { parseDate, type DateValue } from '@internationalized/date'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { CalendarDays } from '@lucide/vue'
import { Calendar } from '@/components/ui/calendar'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { formatDateText, getWeekStart, parseDateText } from '@/lib/time'
import { cn } from '@/lib/utils'
import { useSettingsStore } from '@/stores/settings'

// Date field in the date format setting, with the calendar (shadcn-vue)
// behind the calendar button. The native date input would show the OS
// locale's format instead.
const model = defineModel<string>({ required: true }) // `YYYY-MM-DD`
const props = defineProps<{ class?: string; id?: string; required?: boolean; min?: string; ariaLabel?: string }>()

const settings = useSettingsStore()
const text = ref('')
const invalid = ref(false)
const pickerOpen = ref(false)

const { locale } = useI18n()
const placeholder = () => {
  const [d, m, y] = locale.value === 'de' ? ['TT', 'MM', 'JJJJ'] : ['DD', 'MM', 'YYYY']
  return { dmy: `${d}.${m}.${y}`, mdy: `${m}/${d}/${y}`, ymd: `${y}-${m}-${d}` }[settings.dateOrder]
}
const show = () => {
  text.value = model.value ? formatDateText(model.value, settings.dateOrder) : ''
  invalid.value = false
}
watch([model, () => settings.dateOrder], show, { immediate: true })

function commit() {
  if (text.value.trim() === '') {
    if (!props.required) model.value = ''
    show()
    return
  }
  const parsed = parseDateText(text.value, settings.dateOrder)
  if (parsed === null || (props.min && parsed < props.min)) {
    invalid.value = true
    return
  }
  model.value = parsed
  show()
}

/** The calendar's value: `YYYY-MM-DD` as a date (none when empty or not a date). */
const picked = computed<DateValue | undefined>(() => {
  try {
    return model.value ? parseDate(model.value) : undefined
  } catch {
    return undefined
  }
})
const minValue = computed<DateValue | undefined>(() => {
  try {
    return props.min ? parseDate(props.min) : undefined
  } catch {
    return undefined
  }
})
function pick(value: DateValue | undefined) {
  if (!value) return
  model.value = value.toString()
  pickerOpen.value = false
}
</script>

<template>
  <div :class="cn('relative', props.class)">
    <Input
      :id="id"
      v-model="text"
      class="pr-9"
      autocomplete="off"
      :required="required"
      :placeholder="placeholder()"
      :aria-label="ariaLabel"
      :aria-invalid="invalid || undefined"
      @blur="commit"
      @keydown.enter="commit"
    />
    <Popover v-model:open="pickerOpen">
      <PopoverTrigger as-child>
        <button
          type="button"
          class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-muted-foreground hover:text-foreground"
          tabindex="-1"
          :aria-label="$t('common.pickDate')"
        >
          <CalendarDays class="size-4" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" class="w-auto p-0">
        <Calendar
          :model-value="picked"
          :min-value="minValue"
          :locale="locale"
          :week-starts-on="(getWeekStart() as 0 | 1 | 2 | 3 | 4 | 5 | 6)"
          initial-focus
          @update:model-value="pick"
        />
      </PopoverContent>
    </Popover>
  </div>
</template>
