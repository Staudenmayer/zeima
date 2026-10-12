<script setup lang="ts">
// Bare windows (quick switch, prompt). On Windows they are transparent so
// the page's rounded corners show and the page paints the background. On
// Linux and macOS the window is opaque (see src-tauri/src/windows.rs), so
// the panel fills it with square corners instead.
const transparent = navigator.userAgent.includes('Windows')
document.documentElement.classList.add(transparent ? 'bare' : 'opaque-window')
</script>

<template>
  <div class="h-screen overflow-hidden">
    <slot />
  </div>
</template>
