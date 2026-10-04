<template>
  <div v-if="false" class="flex items-center gap-1 rounded-lg border p-1">
    <template v-for="theme in themes" :key="theme.value">
      <button v-if="colorMode !== theme.value" type="button" :class="[
        'rounded-md p-2 transition-colors',
        colorMode === theme.value
          ? 'bg-muted'
          : 'hover:bg-muted/50',
      ]" :title="theme.label" @click="colorMode = theme.value">
        <component :is="theme.icon" class="size-4" />
        <span class="sr-only">{{ theme.label }}</span>
      </button>
    </template>
  </div>
</template>

<script setup lang="ts">
import { useColorMode } from '@vueuse/core'
import { LucideIcon, Moon, Sun } from '@lucide/vue';

const colorMode = useColorMode()

interface Theme {
  value: 'light' | 'dark' | 'auto';
  label: 'Light' | 'Dark' | 'System';
  icon: LucideIcon;
}

const themes: Theme[] = [
  {
    value: 'light',
    label: 'Light',
    icon: Sun,
  },
  {
    value: 'dark',
    label: 'Dark',
    icon: Moon,
  }
]
</script>
