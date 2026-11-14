<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Check, ClipboardCheck, X } from '@lucide/vue'
import { commands, type Absence } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { useApprovalsStore } from '@/stores/approvals'
import { useOrgStore } from '@/stores/org'
import { useWorkStore } from '@/stores/work'
import { useWorkspaceStore } from '@/stores/workspace'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'

definePage({ meta: { titleKey: 'nav.approvals' } })

// Only absences are approved: someone with the right decides the requests of
// their people. (Time entries are not handed in for approval.)
const format = useFormat()
const org = useOrgStore()
const workspaces = useWorkspaceStore()
const approvals = useApprovalsStore()
const work = useWorkStore()

const requests = computed(() => approvals.absenceRequests)
const nameOf = (id: string) => org.nameOf(id)
const day = (iso: string) => format.shortDate(Date.parse(`${iso}T12:00:00`))
const range = (a: Absence) => (a.startDate === a.endDate ? day(a.startDate) : `${day(a.startDate)} – ${day(a.endDate)}`)
const typeName = (a: Absence) => {
  const type = work.absenceTypesById.get(a.typeId)
  return type ? work.absenceTypeName(type) : ''
}

async function load() {
  if (!workspaces.inOrg) return
  try {
    await approvals.load()
    if (org.members.length === 0) await org.load().catch(() => {})
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch(() => workspaces.active?.id, load)

async function decide(a: Absence, approve: boolean, why = '') {
  try {
    await call(commands.absenceDecide(a.id, approve, why))
    await approvals.load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const rejecting = ref<Absence | null>(null)
const reason = ref('')
function askReject(a: Absence) {
  rejecting.value = a
  reason.value = ''
}
async function reject() {
  const a = rejecting.value
  if (!a) return
  await decide(a, false, reason.value)
  rejecting.value = null
}

const { sort, sorted: sortedRequests } = useSort(requests, {
  person: (a) => nameOf(a.userId),
  period: (a) => a.startDate,
  type: (a) => typeName(a),
  note: (a) => a.note,
})
</script>

<template>
  <PageHeader :title="$t('nav.approvals')" />

  <EmptyState v-if="!workspaces.inOrg" :icon="ClipboardCheck" :text="$t('approvals.personal')" />
  <EmptyState v-else-if="requests.length === 0" :icon="ClipboardCheck" :text="$t('approvals.none')" />

  <section v-else class="grid gap-2">
    <h2 class="text-sm font-medium">{{ $t('approvals.absenceRequests') }}</h2>
    <div class="overflow-x-auto rounded-lg border">
      <table class="w-full text-sm">
        <thead class="text-left text-xs text-muted-foreground">
          <tr class="border-b">
            <SortHead :sort="sort" column="person" class="px-3 py-2 font-medium">{{ $t('approvals.person') }}</SortHead>
            <SortHead :sort="sort" column="period" class="px-3 py-2 font-medium">{{ $t('absences.period') }}</SortHead>
            <SortHead :sort="sort" column="type" class="px-3 py-2 font-medium">{{ $t('absences.type') }}</SortHead>
            <SortHead :sort="sort" column="note" class="px-3 py-2 font-medium">{{ $t('entry.note') }}</SortHead>
            <th class="w-48 px-1 py-2"><span class="sr-only">{{ $t('approvals.approve') }}</span></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="a in sortedRequests" :key="a.id" class="border-b last:border-b-0 hover:bg-muted/30">
            <td class="whitespace-nowrap px-3 py-1.5 font-medium">{{ nameOf(a.userId) || $t('approvals.someone') }}</td>
            <td class="whitespace-nowrap px-3 py-1.5 tabular-nums">{{ range(a) }}</td>
            <td class="whitespace-nowrap px-3 py-1.5">{{ typeName(a) }}</td>
            <td class="max-w-0 w-full truncate px-3 py-1.5 text-muted-foreground" :title="a.note">{{ a.note }}</td>
            <td class="px-1 py-0.5">
              <div class="flex justify-end gap-1">
                <Button size="sm" @click="decide(a, true)"><Check /> {{ $t('approvals.approve') }}</Button>
                <Button size="sm" variant="outline" @click="askReject(a)"><X /> {{ $t('approvals.reject') }}</Button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>

  <Dialog :open="rejecting !== null" @update:open="(o) => !o && (rejecting = null)">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('approvals.reject') }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="reject">
        <div class="grid gap-2"><Label for="reject-note">{{ $t('approvals.rejectReason') }}</Label><Input id="reject-note" v-model="reason" maxlength="500" autofocus /></div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="rejecting = null">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('approvals.reject') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
