<script setup lang="ts">
import { useColorMode } from '@vueuse/core'
import { type LucideIcon, Monitor, Moon, Sun } from '@lucide/vue'

// Applies immediately; stored per device and shared by all windows.
const colorMode = useColorMode({ emitAuto: true })

const themes: { value: 'light' | 'dark' | 'auto'; label: string; icon: LucideIcon }[] = [
  { value: 'light', label: 'settings.themes.light', icon: Sun },
  { value: 'dark', label: 'settings.themes.dark', icon: Moon },
  { value: 'auto', label: 'settings.themes.system', icon: Monitor },
]
</script>

<template>
  <div class="inline-flex w-fit rounded-lg bg-muted p-1 text-sm" role="radiogroup" :aria-label="$t('settings.theme')">
    <button
      v-for="theme in themes"
      :key="theme.value"
      type="button"
      role="radio"
      :aria-checked="colorMode === theme.value"
      class="flex items-center gap-1.5 rounded-md px-3 py-1.5 transition-colors"
      :class="colorMode === theme.value ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'"
      @click="colorMode = theme.value"
    >
      <component :is="theme.icon" class="size-4" />
      {{ $t(theme.label) }}
    </button>
  </div>
</template>
