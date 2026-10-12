<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Check, ChevronDown } from '@lucide/vue'
import { commands } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { call } from '@/lib/ipc'

const props = defineProps<{ systemZone: string }>()
/** IANA zone; `null` = follow the operating system. */
const model = defineModel<string | null>({ required: true })

const zones = ref<string[]>([])
const open = ref(false)
const search = ref('')
onMounted(async () => {
  zones.value = await call(commands.timezoneList()).catch(() => [])
})

// Up to 200 matches keep the list fast; typing narrows it down.
const matches = computed(() => {
  const q = search.value.trim().toLowerCase().replace(/\s+/g, '_')
  return zones.value.filter((z) => !q || z.toLowerCase().includes(q)).slice(0, 200)
})

function pick(zone: string | null) {
  model.value = zone
  open.value = false
  search.value = ''
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <Button variant="outline" class="w-full justify-between font-normal">
        <span class="truncate">{{ model ?? $t('settings.region.systemZone', { zone: props.systemZone }) }}</span>
        <ChevronDown class="opacity-50" />
      </Button>
    </PopoverTrigger>
    <PopoverContent align="start" class="w-(--reka-popover-trigger-width) p-0">
      <div class="border-b p-2">
        <Input v-model="search" :placeholder="$t('settings.region.searchZone')" autofocus />
      </div>
      <ul class="max-h-64 overflow-y-auto p-1 text-sm">
        <li>
          <button type="button" class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-muted" @click="pick(null)">
            <Check class="size-4" :class="model === null ? '' : 'invisible'" />
            {{ $t('settings.region.systemZone', { zone: props.systemZone }) }}
          </button>
        </li>
        <li v-for="z in matches" :key="z">
          <button type="button" class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-muted" @click="pick(z)">
            <Check class="size-4" :class="model === z ? '' : 'invisible'" />
            {{ z }}
          </button>
        </li>
        <li v-if="matches.length === 0" class="px-2 py-3 text-center text-muted-foreground">{{ $t('settings.region.noZone') }}</li>
      </ul>
    </PopoverContent>
  </Popover>
</template>
