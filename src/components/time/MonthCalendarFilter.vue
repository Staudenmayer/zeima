<script setup lang="ts">
import { CalendarDays, ChevronLeft, ChevronRight, X } from '@lucide/vue'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { buttonVariants } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'

// Months of a year chosen on a calendar: the year's months as a grid (laid out
// like the shadcn-vue calendar), any number of them selected. The year is the
// page's; its arrows change it.
const props = defineProps<{ year: number; label: string }>()
const months = defineModel<(string | null)[]>({ required: true }) // '1'…'12'; empty = every month
const emit = defineEmits<{ 'update:year': [year: number] }>()
const { locale } = useI18n()

const names = computed(() => Array.from({ length: 12 }, (_, i) => new Intl.DateTimeFormat(locale.value, { month: 'short' }).format(new Date(2000, i, 15))))
const longNames = computed(() => Array.from({ length: 12 }, (_, i) => new Intl.DateTimeFormat(locale.value, { month: 'long' }).format(new Date(2000, i, 15))))
const summary = computed(() => {
  if (months.value.length === 0) return props.label
  if (months.value.length === 1) return longNames.value[Number(months.value[0]) - 1] ?? props.label
  return `${props.label} · ${months.value.length}`
})
const on = (i: number) => months.value.includes(String(i + 1))
const toggle = (i: number) => (months.value = on(i) ? months.value.filter((m) => m !== String(i + 1)) : [...months.value, String(i + 1)])
</script>

<template>
  <Popover>
    <PopoverTrigger as-child>
      <Button variant="outline" size="sm" :class="months.length ? 'border-primary/60' : ''">
        <CalendarDays /> {{ summary }}
      </Button>
    </PopoverTrigger>
    <PopoverContent align="start" class="w-auto p-3">
      <div class="flex items-center justify-between pt-1">
        <Button variant="outline" :class="cn(buttonVariants({ variant: 'outline' }), 'size-7 bg-transparent p-0 opacity-50 hover:opacity-100')" :aria-label="$t('timesheet.previous')" @click="emit('update:year', year - 1)"><ChevronLeft class="size-4" /></Button>
        <span class="text-sm font-medium tabular-nums">{{ year }}</span>
        <Button variant="outline" :class="cn(buttonVariants({ variant: 'outline' }), 'size-7 bg-transparent p-0 opacity-50 hover:opacity-100')" :aria-label="$t('timesheet.next')" @click="emit('update:year', year + 1)"><ChevronRight class="size-4" /></Button>
      </div>
      <div class="mt-4 grid grid-cols-4 gap-1">
        <button
          v-for="(name, i) in names"
          :key="i"
          type="button"
          :aria-pressed="on(i)"
          :class="cn(buttonVariants({ variant: 'ghost' }), 'h-9 w-14 p-0 font-normal', on(i) && 'bg-primary text-primary-foreground hover:bg-primary hover:text-primary-foreground')"
          @click="toggle(i)"
        >{{ name }}</button>
      </div>
      <Button v-if="months.length" variant="ghost" size="sm" class="mt-2 w-full" @click="months = []"><X /> {{ $t('reports.clearFilter') }}</Button>
    </PopoverContent>
  </Popover>
</template>
