<script setup lang="ts">
import type { Component } from 'vue'

defineProps<{ icon?: Component; text: string }>()
</script>

<template>
  <div class="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed p-10 text-center">
    <component :is="icon" v-if="icon" class="size-8 text-muted-foreground" />
    <p class="max-w-sm text-sm text-muted-foreground">{{ text }}</p>
    <slot />
  </div>
</template>
