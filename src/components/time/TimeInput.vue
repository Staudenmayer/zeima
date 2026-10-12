<script setup lang="ts">
import { ref, watch } from 'vue'
import { Input } from '@/components/ui/input'
import { formatTimeText, parseTimeText } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'

// Time field in the 12/24-hour format setting. The native time input
// would follow the OS locale instead. Accepts 14:30, 1430, 2:30 pm, 9 …
const model = defineModel<string>({ required: true }) // `HH:MM`, 24 h
defineOptions({ inheritAttrs: false })

const settings = useSettingsStore()
const text = ref('')
const invalid = ref(false)

const show = () => {
  text.value = model.value ? formatTimeText(model.value, settings.hour12) : ''
  invalid.value = false
}
watch([model, () => settings.hour12], show, { immediate: true })

function commit() {
  if (text.value.trim() === '') {
    model.value = ''
    invalid.value = false
    return
  }
  const parsed = parseTimeText(text.value)
  if (parsed === null) {
    invalid.value = true
    return
  }
  model.value = parsed
  show()
}
</script>

<template>
  <Input
    v-bind="$attrs"
    v-model="text"
    inputmode="numeric"
    autocomplete="off"
    :placeholder="settings.hour12 ? '9:00 AM' : '09:00'"
    :aria-invalid="invalid || undefined"
    @blur="commit"
    @keydown.enter="commit"
  />
</template>
