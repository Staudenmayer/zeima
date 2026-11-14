<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import type { ManagerVisibility, OrgPolicy, PolicySettings, PresenceVisibility } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { errorMessage } from '@/lib/ipc'

const props = defineProps<{ title: string; policy: OrgPolicy | null; save: (settings: PolicySettings) => Promise<unknown> }>()
const open = defineModel<boolean>('open', { required: true })

const MANAGER: ManagerVisibility[] = ['full', 'hide_notes', 'totals_only']
const PRESENCE: PresenceVisibility[] = ['org', 'off']

const form = ref<PolicySettings>({})
/** The target is edited in hours, stored in minutes. */
const targetHours = ref<number | null>(null)
const idleMinutes = ref<number | null>(null)
const saving = ref(false)

watch(open, (isOpen) => {
  if (!isOpen) return
  const p = props.policy
  form.value = {
    requireProject: p?.requireProject ?? false,
    requireTags: p?.requireTags ?? false,
    managerVisibility: p?.managerVisibility ?? 'full',
    presenceVisibility: p?.presenceVisibility ?? 'org',
  }
  targetHours.value = p?.weeklyTargetMinutes ? p.weeklyTargetMinutes / 60 : null
  idleMinutes.value = p?.autoPauseIdleMinutes ?? null
})

async function submit() {
  saving.value = true
  try {
    await props.save({
      ...form.value,
      weeklyTargetMinutes: targetHours.value ? Math.round(Number(targetHours.value) * 60) : null,
      autoPauseIdleMinutes: idleMinutes.value ? Math.round(Number(idleMinutes.value)) : null,
    })
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-lg">
      <DialogHeader><DialogTitle>{{ title }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="submit">
        <div class="flex items-center justify-between gap-4">
          <div><Label>{{ $t('org.policy.requireProject') }}</Label><p class="text-xs text-muted-foreground">{{ $t('org.policy.requireProjectHint') }}</p></div>
          <Switch :model-value="form.requireProject ?? false" @update:model-value="form.requireProject = $event" />
        </div>
        <div class="flex items-center justify-between gap-4">
          <div><Label>{{ $t('org.policy.requireTags') }}</Label><p class="text-xs text-muted-foreground">{{ $t('org.policy.requireTagsHint') }}</p></div>
          <Switch :model-value="form.requireTags ?? false" @update:model-value="form.requireTags = $event" />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('org.policy.managerVisibility') }}</Label>
          <Select v-model="form.managerVisibility">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem v-for="m in MANAGER" :key="m" :value="m">{{ $t(`org.policy.manager.${m}`) }}</SelectItem></SelectContent>
          </Select>
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('org.policy.presenceVisibility') }}</Label>
          <Select v-model="form.presenceVisibility">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem v-for="m in PRESENCE" :key="m" :value="m">{{ $t(`org.policy.presence.${m}`) }}</SelectItem></SelectContent>
          </Select>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2"><Label for="pol-target">{{ $t('org.policy.weeklyTarget') }}</Label><Input id="pol-target" :model-value="targetHours ?? ''" @update:model-value="targetHours = $event === '' ? null : Number($event)" type="number" min="0" max="168" step="0.5" /></div>
          <div class="grid gap-2"><Label for="pol-idle">{{ $t('org.policy.autoPause') }}</Label><Input id="pol-idle" :model-value="idleMinutes ?? ''" @update:model-value="idleMinutes = $event === '' ? null : Number($event)" type="number" min="1" max="600" /></div>
        </div>
        <p class="text-xs text-muted-foreground">{{ $t('org.policy.stricterHint') }}</p>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
