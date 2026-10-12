<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useColorMode } from '@vueuse/core'
import 'vue-sonner/style.css'
import { Toaster } from '@/components/ui/sonner'
import AuthLayout from './layouts/auth.vue'
import BareLayout from './layouts/bare.vue'
import DefaultLayout from './layouts/default.vue'
import PrintLayout from './layouts/print.vue'

// Applies the stored theme (light/dark/system) in every window, including
// the quick-switch and prompt windows that have no theme switch.
useColorMode()

const route = useRoute()

const layout = computed(() => {
  switch (route.meta.layout) {
    case 'auth':
      return AuthLayout
    case 'bare':
      return BareLayout
    case 'print':
      return PrintLayout
    default:
      return DefaultLayout
  }
})
</script>

<template>
  <component :is="layout">
    <RouterView />
  </component>
  <Toaster rich-colors position="bottom-right" />
</template>
