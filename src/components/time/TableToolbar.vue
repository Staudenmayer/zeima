<script setup lang="ts">
import { Search, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

// The bar above a list table: a search box, the table's filters (default
// slot) and "Clear filters" while anything narrows the list.
defineProps<{ active: boolean; placeholder?: string }>()
defineEmits<{ clear: [] }>()
const search = defineModel<string>({ required: true })
</script>

<template>
  <div class="mb-3 flex flex-wrap items-center gap-2">
    <div class="relative">
      <Search class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input v-model="search" class="h-8 w-64 pl-8" :placeholder="placeholder ?? $t('table.search')" :aria-label="placeholder ?? $t('table.search')" />
    </div>
    <slot />
    <Button v-if="active" size="sm" variant="ghost" @click="$emit('clear')"><X /> {{ $t('reports.clearFilter') }}</Button>
  </div>
</template>
