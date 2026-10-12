<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { CalendarDays } from '@lucide/vue'
import { Input } from '@/components/ui/input'
import { formatDateText, parseDateText } from '@/lib/time'
import { cn } from '@/lib/utils'
import { useSettingsStore } from '@/stores/settings'

// Date field in the date format setting, with the system date picker
// behind the calendar button. The native date input alone would show the
// OS locale's format instead.
const model = defineModel<string>({ required: true }) // `YYYY-MM-DD`
const props = defineProps<{ class?: string; id?: string; required?: boolean; min?: string; ariaLabel?: string }>()

const settings = useSettingsStore()
const text = ref('')
const invalid = ref(false)
const picker = ref<HTMLInputElement | null>(null)

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

function openPicker() {
  const el = picker.value
  if (!el) return
  try {
    el.showPicker()
  } catch {
    el.focus()
  }
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
    <button
      type="button"
      class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-muted-foreground hover:text-foreground"
      tabindex="-1"
      @click="openPicker"
    >
      <CalendarDays class="size-4" />
    </button>
    <!-- Only for the picker; the visible field is the text input. -->
    <input
      ref="picker"
      type="date"
      class="pointer-events-none absolute right-0 bottom-0 size-0 opacity-0"
      tabindex="-1"
      aria-hidden="true"
      :value="model"
      :min="min"
      @change="model = ($event.target as HTMLInputElement).value"
    />
  </div>
</template>
