<script setup lang="ts">
import PersonCard from '@/components/time/PersonCard.vue'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

// Wraps whatever opens a person's profile (a row, a name): a click shows the profile card beside it.
defineProps<{ userId: string; side?: 'left' | 'right' | 'top' | 'bottom' }>()
</script>

<template>
  <Popover>
    <PopoverTrigger as-child><slot /></PopoverTrigger>
    <PopoverContent :side="side ?? 'left'" align="start" :side-offset="8" :collision-padding="8" class="w-80 overflow-hidden p-0">
      <PersonCard :user-id="userId" />
    </PopoverContent>
  </Popover>
</template>
