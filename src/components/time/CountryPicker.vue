<script setup lang="ts">
import { computed } from 'vue'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { localized, useWorkStore } from '@/stores/work'

const profileId = defineModel<string>('profileId', { required: true })
const region = defineModel<string | null>('region', { required: true })

const work = useWorkStore()
// Select items cannot have an empty value.
const WHOLE = '__whole__'

const profile = computed(() => work.profiles.find((p) => p.id === profileId.value))
const regionValue = computed({
  get: () => region.value ?? WHOLE,
  set: (v: string) => (region.value = v === WHOLE ? null : v),
})

function selectProfile(id: string) {
  profileId.value = id
  region.value = null
}
</script>

<template>
  <div class="grid gap-3 sm:grid-cols-2">
    <div class="grid gap-2">
      <Label>{{ $t('work.profile') }}</Label>
      <Select :model-value="profileId" @update:model-value="selectProfile(String($event))">
        <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem v-for="p in work.profiles" :key="p.id" :value="p.id">{{ localized(p.name) }}</SelectItem>
        </SelectContent>
      </Select>
    </div>
    <div class="grid gap-2">
      <Label>{{ $t('work.region') }}</Label>
      <Select v-model="regionValue" :disabled="!profile?.regions.length">
        <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem :value="WHOLE">{{ $t('work.noRegion') }}</SelectItem>
          <SelectItem v-for="r in profile?.regions ?? []" :key="r.id" :value="r.id">{{ localized(r.name) }}</SelectItem>
        </SelectContent>
      </Select>
    </div>
  </div>
</template>
