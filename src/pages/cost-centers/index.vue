<script setup lang="ts">
import { useWorkspaceStore } from '@/stores/workspace'
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Landmark, Pencil, Plus, Trash2 } from '@lucide/vue'
import { commands, type CostCenter } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import CostCenterDialog from '@/components/time/CostCenterDialog.vue'
import CostCenterSelect from '@/components/time/CostCenterSelect.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { parseDate, startOfDay, toDateInput } from '@/lib/time'
import { costCenterLabel, useCostCentersStore } from '@/stores/costCenters'

definePage({ meta: { titleKey: 'nav.costCenters' } })

const workspaces = useWorkspaceStore()

const costCenters = useCostCentersStore()
const format = useFormat()

/** Depth-first by code, children under their parent. */
const tree = computed(() => {
  const children = new Map<string | null, CostCenter[]>()
  for (const c of costCenters.all) {
    const parent = c.parentId && costCenters.byId.has(c.parentId) ? c.parentId : null
    children.set(parent, [...(children.get(parent) ?? []), c])
  }
  const out: { cc: CostCenter; depth: number }[] = []
  const walk = (parent: string | null, depth: number) => {
    for (const cc of (children.get(parent) ?? []).sort((a, b) => a.code.localeCompare(b.code, undefined, { numeric: true }))) {
      out.push({ cc, depth })
      walk(cc.id, depth + 1)
    }
  }
  walk(null, 0)
  return out
})

function validity(c: CostCenter) {
  const d = (s: string) => format.longDate(parseDate(s))
  if (c.validFrom && c.validTo) return `${d(c.validFrom)} – ${d(c.validTo)}`
  if (c.validFrom) return `${d(c.validFrom)} –`
  if (c.validTo) return `– ${d(c.validTo)}`
  return ''
}

const dialogOpen = ref(false)
const editing = ref<CostCenter | null>(null)
function open(c: CostCenter | null) {
  editing.value = c
  dialogOpen.value = true
}

const deleting = ref<CostCenter | null>(null)
const confirmOpen = ref(false)
function askDelete(c: CostCenter) {
  deleting.value = c
  confirmOpen.value = true
}
async function confirmDelete() {
  if (!deleting.value) return
  try {
    await costCenters.remove(deleting.value.id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

// The user's default cost center: the assignment in effect today. Picking
// another one applies from today on and keeps the older lines as history.
const today = () => toDateInput(Date.now())
const currentDefault = computed(() => {
  const t = today()
  return costCenters.assignments.find((a) => a.validFrom <= t && (!a.validTo || t <= a.validTo)) ?? null
})
const defaultId = computed({
  get: () => currentDefault.value?.costCenterId ?? null,
  set: (id: string | null) => void setDefault(id),
})
async function setDefault(id: string | null) {
  const t = today()
  const current = currentDefault.value
  if (id === (current?.costCenterId ?? null)) return
  try {
    if (id === null) {
      // Clearing: end the line today, or drop it when it only started today.
      if (current && current.validFrom >= t) await call(commands.costCenterAssignmentDelete(current.id))
      else if (current) {
        const yesterday = toDateInput(startOfDay(Date.now()) - 1)
        await call(commands.costCenterAssignmentSave(current.id, { costCenterId: current.costCenterId, validFrom: current.validFrom, validTo: yesterday }))
      }
    } else if (current && current.validFrom === t) {
      await call(commands.costCenterAssignmentSave(current.id, { costCenterId: id, validFrom: t, validTo: current.validTo }))
    } else {
      await call(commands.costCenterAssignmentSave(null, { costCenterId: id, validFrom: t, validTo: null }))
    }
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

// Long lists are paged.
const PAGE_SIZE = 6
const page = ref(0)
const pageCount = computed(() => Math.max(1, Math.ceil(tree.value.length / PAGE_SIZE)))
const pageRows = computed(() => {
  const p = Math.min(page.value, pageCount.value - 1)
  return tree.value.slice(p * PAGE_SIZE, (p + 1) * PAGE_SIZE)
})
const pageNo = computed(() => Math.min(page.value, pageCount.value - 1))
</script>

<template>
  <PageHeader :title="$t('nav.costCenters')">
    <Button v-if="workspaces.canEditShared" @click="open(null)"><Plus /> {{ $t('costCenters.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="costCenters.all.length === 0" :icon="Landmark" :text="$t('costCenters.empty')">
    <Button v-if="workspaces.canEditShared" variant="outline" @click="open(null)"><Plus /> {{ $t('costCenters.new') }}</Button>
  </EmptyState>

  <div v-else class="grid gap-6">
    <section class="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-card px-4 py-3">
      <div class="min-w-56 flex-1">
        <h2 class="text-sm font-medium">{{ $t('costCenters.myDefault') }}</h2>
        <p class="text-sm text-muted-foreground">
          {{ $t('costCenters.myDefaultHint') }}
          {{
            currentDefault
              ? $t('costCenters.since', { date: format.longDate(parseDate(currentDefault.validFrom)) })
              : $t('costCenters.noneSet')
          }}
        </p>
      </div>
      <div class="w-72 max-w-full">
        <CostCenterSelect v-model="defaultId" :empty-label="$t('costCenters.choose')" />
      </div>
    </section>

    <ul class="divide-y rounded-lg border">
      <li
        v-for="{ cc, depth } in pageRows"
        :key="cc.id"
        class="group flex items-center gap-3 px-3 py-2 text-sm hover:bg-muted/40"
        :class="cc.active ? '' : 'text-muted-foreground'"
        @dblclick="open(cc)"
      >
        <span class="w-24 shrink-0 font-mono text-xs" :style="{ paddingLeft: `${depth * 1}rem` }">{{ cc.code }}</span>
        <span class="flex-1 truncate">{{ cc.name }}</span>
        <span class="text-xs text-muted-foreground tabular-nums">{{ validity(cc) }}</span>
        <Badge v-if="cc.id === currentDefault?.costCenterId" variant="outline">{{ $t('costCenters.default') }}</Badge>
        <Badge v-if="!cc.active" variant="secondary">{{ $t('costCenters.inactive') }}</Badge>
        <div class="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <Button v-if="workspaces.canEditShared" size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(cc)"><Pencil /></Button>
          <Button v-if="workspaces.canEditShared" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(cc)"><Trash2 /></Button>
        </div>
      </li>
    </ul>

    <div v-if="pageCount > 1" class="-mt-2 flex items-center justify-between gap-3 text-sm">
      <span class="tabular-nums text-muted-foreground">
        {{ $t('costCenters.pageOf', { page: pageNo + 1, pages: pageCount, count: tree.length }) }}
      </span>
      <div class="flex gap-2">
        <Button size="sm" variant="outline" :disabled="pageNo === 0" @click="page = pageNo - 1">{{ $t('costCenters.previous') }}</Button>
        <Button size="sm" variant="outline" :disabled="pageNo >= pageCount - 1" @click="page = pageNo + 1">{{ $t('costCenters.next') }}</Button>
      </div>
    </div>
  </div>

  <CostCenterDialog v-model:open="dialogOpen" :cost-center="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('costCenters.deleteConfirm', { name: deleting ? costCenterLabel(deleting) : '' })"
    @confirm="confirmDelete"
  />
</template>
