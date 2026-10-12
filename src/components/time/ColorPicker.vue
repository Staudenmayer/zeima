<script setup lang="ts">
import { Check } from '@lucide/vue'
import { PALETTE } from '@/lib/colors'

const color = defineModel<string>({ required: true })
</script>

<template>
  <div class="flex flex-wrap items-center gap-1.5">
    <button
      v-for="c in PALETTE"
      :key="c"
      type="button"
      class="flex size-6 items-center justify-center rounded-full ring-offset-2 ring-offset-background focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      :style="{ backgroundColor: c }"
      :aria-label="c"
      @click="color = c"
    >
      <Check v-if="color === c" class="size-3.5 text-white" />
    </button>
    <input v-model="color" type="color" class="size-6 cursor-pointer rounded border-0 bg-transparent p-0" />
  </div>
</template>
