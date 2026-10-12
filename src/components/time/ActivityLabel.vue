<script setup lang="ts">
import type { Activity } from '@/bindings'

defineProps<{ activity: Activity | null | undefined }>()
</script>

<template>
  <span class="inline-flex min-w-0 items-center gap-2">
    <span
      class="size-2.5 shrink-0 rounded-sm"
      :class="activity ? '' : 'border border-dashed border-muted-foreground'"
      :style="activity ? { backgroundColor: activity.color } : undefined"
    />
    <span class="truncate" :class="activity ? '' : 'text-muted-foreground'">
      {{ activity ? activity.name : $t('activities.none') }}
    </span>
  </span>
</template>
