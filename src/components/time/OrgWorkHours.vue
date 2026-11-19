<script setup lang="ts">
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import WorkRecords from '@/components/time/WorkRecords.vue'
import { errorMessage } from '@/lib/ipc'
import { useOrgStore } from '@/stores/org'
import { useWorkspaceStore } from '@/stores/workspace'

/**
 * Organization → Work hours: the schedule, vacation entitlement, break rules and overtime corrections of every member
 * (the organization's records, set by whoever manages members). They are kept in the organization's workspace, so the
 * app has to be working in it; the server decides who may write them.
 */
const org = useOrgStore()
const workspace = useWorkspaceStore()

const picked = ref<string | null>(null)
const subject = computed(() => picked.value ?? org.myId ?? '')
/** The shared workspace of the organization being looked at. */
const orgWorkspace = computed(() => workspace.all.find((w) => w.kind === 'org' && w.orgId === org.activeId))
const inThisOrg = computed(() => workspace.active?.id === orgWorkspace.value?.id && !!orgWorkspace.value)

async function open() {
  if (!orgWorkspace.value) return
  try {
    await workspace.switchTo(orgWorkspace.value.id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <div class="grid max-w-4xl gap-6 pt-4">
    <div v-if="!inThisOrg" class="flex flex-wrap items-center gap-3 rounded-lg border bg-muted/50 p-4 text-sm">
      <span class="text-muted-foreground">{{ $t('work.openOrgWorkspace') }}</span>
      <Button size="sm" :disabled="!orgWorkspace" @click="open">{{ $t('work.openOrgWorkspaceButton') }}</Button>
    </div>
    <template v-else>
      <div class="flex flex-wrap items-center gap-3 rounded-lg border p-3">
        <Label>{{ $t('work.managedPerson') }}</Label>
        <Select :model-value="subject" @update:model-value="(v) => (picked = String(v))">
          <SelectTrigger class="w-72"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem v-for="m in org.members" :key="m.id" :value="m.id">{{ m.name || m.email || m.id }}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <WorkRecords :user-id="subject" managed />
    </template>
  </div>
</template>
