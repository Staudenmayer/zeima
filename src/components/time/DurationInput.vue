<script setup lang="ts">
import { ref, watch } from 'vue'
import { Input } from '@/components/ui/input'
import { hoursToMinutes, minutesToHours } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'

// Hours field in the duration format setting. With hours:minutes, `7,45`
// means 7:45; with decimal hours it means 7.45 h. When the field is left,
// it shows how the value was read (`7:45` / `7,75`).
const model = defineModel<number | null>({ required: true }) // minutes
const props = defineProps<{ signed?: boolean }>()
defineOptions({ inheritAttrs: false })

const settings = useSettingsStore()
const text = ref('')
const invalid = ref(false)

const show = () => {
  text.value = model.value === null ? '' : minutesToHours(model.value, settings.durationFormat, settings.decimalComma)
  if (props.signed && model.value !== null && model.value > 0) text.value = `+${text.value}`
  invalid.value = false
}
watch([model, () => settings.durationFormat, () => settings.decimalComma], show, { immediate: true })

/** Parses the current text; also used for the live week total. */
function parse(): number | null {
  const minutes = hoursToMinutes(text.value, settings.durationFormat)
  return minutes !== null && !props.signed && minutes < 0 ? null : minutes
}

function commit() {
  if (text.value.trim() === '') {
    model.value = null
    invalid.value = false
    return
  }
  const minutes = parse()
  if (minutes === null) {
    invalid.value = true
    return
  }
  model.value = minutes
  show()
}
</script>

<template>
  <Input
    v-bind="$attrs"
    v-model="text"
    inputmode="decimal"
    autocomplete="off"
    :placeholder="settings.durationFormat === 'hm' ? (signed ? '+2:30' : '7:45') : settings.decimalComma ? '7,75' : '7.75'"
    :aria-invalid="invalid || undefined"
    @blur="commit"
    @keydown.enter="commit"
  />
</template>
