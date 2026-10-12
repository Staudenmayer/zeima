<script setup lang="ts">
import type { Project } from '@/bindings'

defineProps<{ project: Project | null | undefined; showAlias?: boolean }>()
</script>

<template>
  <span class="inline-flex min-w-0 items-center gap-2">
    <span
      class="size-2.5 shrink-0 rounded-full"
      :class="project ? '' : 'border border-dashed border-muted-foreground'"
      :style="project ? { backgroundColor: project.color } : undefined"
    />
    <span class="truncate" :class="project ? '' : 'text-muted-foreground'">
      {{ project ? project.name : $t('common.noProject') }}
    </span>
    <span v-if="showAlias && project?.alias" class="shrink-0 font-mono text-xs text-muted-foreground">
      {{ project.alias }}
    </span>
  </span>
</template>
