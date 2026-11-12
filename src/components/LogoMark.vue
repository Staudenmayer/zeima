<script setup lang="ts">
// `onBlue`: all white, for use on the brand-blue tile (the app icon).
defineProps<{ onBlue?: boolean }>()
</script>

<template>
  <!-- Zeima mark: a stopwatch whose face holds a Z. On its own the ring is the
       brand blue (lighter on dark) and the Z follows the text color. -->
  <svg viewBox="0 0 64 64" fill="none" role="img" :aria-label="$t('app.name')" class="shrink-0">
    <g :class="onBlue ? 'stroke-white' : 'stroke-blue-600 dark:stroke-blue-400'" stroke-width="5" stroke-linecap="round">
      <circle cx="32" cy="36" r="24" />
      <path d="M32 5v7M26 5h12" />
    </g>
    <path d="M23 29h18L23 44h18" :class="onBlue ? 'stroke-white' : 'stroke-foreground'" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</template>
