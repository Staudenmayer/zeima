<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, X } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

// A form field that picks several of a list: a button that opens the list
// (with a search box when it is long), and the picks as removable chips below.
export interface MultiOption {
  value: string
  label: string
  /** Nesting depth, to indent a tree (departments). */
  depth?: number
  /** The value is a person's id: their profile picture is shown. */
  person?: boolean
}

const props = defineProps<{ options: MultiOption[]; placeholder: string; empty?: string; searchPlaceholder?: string }>()
const model = defineModel<string[]>({ required: true })

const query = ref('')
const SEARCH_FROM = 8
const shown = computed(() => {
  const q = query.value.trim().toLowerCase()
  return q ? props.options.filter((o) => o.label.toLowerCase().includes(q)) : props.options
})
const picked = computed(() => model.value.map((v) => props.options.find((o) => o.value === v)).filter((o): o is MultiOption => o !== undefined))
const toggle = (value: string, on: boolean) => (model.value = on ? [...new Set([...model.value, value])] : model.value.filter((v) => v !== value))
</script>

<template>
  <div class="grid gap-2">
    <Popover>
      <PopoverTrigger as-child>
        <Button type="button" variant="outline" class="w-full justify-between font-normal" :class="picked.length ? '' : 'text-muted-foreground'">
          <span class="truncate">{{ picked.length ? picked.map((o) => o.label).join(', ') : placeholder }}</span>
          <ChevronDown class="opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" class="w-72 p-2">
        <Input v-if="options.length >= SEARCH_FROM" v-model="query" class="mb-2 h-8" :placeholder="searchPlaceholder ?? ''" />
        <div class="grid max-h-56 gap-0.5 overflow-y-auto">
          <label
            v-for="o in shown"
            :key="o.value"
            class="flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1.5 text-sm hover:bg-accent"
            :style="{ paddingLeft: `${0.5 + (o.depth ?? 0)}rem` }"
          >
            <Checkbox :model-value="model.includes(o.value)" @update:model-value="toggle(o.value, $event === true)" />
            <UserAvatar v-if="o.person" :user-id="o.value" :name="o.label" size="xs" />
            <span class="truncate">{{ o.label }}</span>
          </label>
          <p v-if="shown.length === 0" class="px-2 py-1.5 text-sm text-muted-foreground">{{ empty ?? '' }}</p>
        </div>
      </PopoverContent>
    </Popover>
    <div v-if="picked.length" class="flex flex-wrap gap-1">
      <Badge v-for="o in picked" :key="o.value" variant="secondary" class="gap-1">
        <UserAvatar v-if="o.person" :user-id="o.value" :name="o.label" size="xs" />
        {{ o.label }}
        <button type="button" class="opacity-60 hover:opacity-100" :aria-label="$t('common.delete')" @click="toggle(o.value, false)"><X class="size-3" /></button>
      </Badge>
    </div>
  </div>
</template>
