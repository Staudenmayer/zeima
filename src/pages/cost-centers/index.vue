<script setup lang="ts">
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Landmark, Pencil, Plus, Trash2 } from '@lucide/vue'
import { commands, type CostCenter, type CostCenterAssignment } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import DateInput from '@/components/time/DateInput.vue'
import { Label } from '@/components/ui/label'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import CostCenterDialog from '@/components/time/CostCenterDialog.vue'
import CostCenterSelect from '@/components/time/CostCenterSelect.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { parseDate, toDateInput } from '@/lib/time'
import { costCenterLabel, useCostCentersStore } from '@/stores/costCenters'

definePage({ meta: { titleKey: 'nav.costCenters' } })

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

// The user's default cost center over time.
const newDefault = ref<string | null>(null)
const newFrom = ref(toDateInput(Date.now()))
async function addDefault() {
  if (!newDefault.value) return
  try {
    await call(commands.costCenterAssignmentSave(null, { costCenterId: newDefault.value, validFrom: newFrom.value, validTo: null }))
    newDefault.value = null
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function removeDefault(a: CostCenterAssignment) {
  try {
    await call(commands.costCenterAssignmentDelete(a.id))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.costCenters')">
    <Button @click="open(null)"><Plus /> {{ $t('costCenters.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="costCenters.all.length === 0" :icon="Landmark" :text="$t('costCenters.empty')">
    <Button variant="outline" @click="open(null)"><Plus /> {{ $t('costCenters.new') }}</Button>
  </EmptyState>

  <div v-else class="grid gap-8">
    <ul class="divide-y rounded-lg border">
      <li
        v-for="{ cc, depth } in tree"
        :key="cc.id"
        class="group flex items-center gap-3 px-3 py-2 text-sm hover:bg-muted/40"
        :class="cc.active ? '' : 'text-muted-foreground'"
        @dblclick="open(cc)"
      >
        <span class="w-24 shrink-0 font-mono text-xs" :style="{ paddingLeft: `${depth * 1}rem` }">{{ cc.code }}</span>
        <span class="flex-1 truncate">{{ cc.name }}</span>
        <span class="text-xs text-muted-foreground tabular-nums">{{ validity(cc) }}</span>
        <Badge v-if="!cc.active" variant="secondary">{{ $t('costCenters.inactive') }}</Badge>
        <div class="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(cc)"><Pencil /></Button>
          <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(cc)"><Trash2 /></Button>
        </div>
      </li>
    </ul>

    <section class="grid gap-3">
      <div>
        <h2 class="font-medium">{{ $t('costCenters.myDefault') }}</h2>
        <p class="text-sm text-muted-foreground">{{ $t('costCenters.myDefaultHint') }}</p>
      </div>
      <ul v-if="costCenters.assignments.length" class="divide-y rounded-lg border">
        <li v-for="a in costCenters.assignments" :key="a.id" class="group flex items-center gap-3 px-3 py-2 text-sm">
          <span class="w-32 shrink-0 tabular-nums text-muted-foreground">
            {{ $t('costCenters.from') }} {{ format.longDate(parseDate(a.validFrom)) }}
          </span>
          <span class="flex-1 truncate">{{ costCenterLabel(costCenters.byId.get(a.costCenterId)) }}</span>
          <Button size="icon" variant="ghost" class="size-7 opacity-0 group-hover:opacity-100 focus-visible:opacity-100" :title="$t('common.delete')" @click="removeDefault(a)"><Trash2 /></Button>
        </li>
      </ul>
      <form class="flex flex-wrap items-end gap-3" @submit.prevent="addDefault">
        <div class="grid min-w-56 flex-1 gap-2">
          <Label>{{ $t('costCenters.costCenter') }}</Label>
          <CostCenterSelect v-model="newDefault" :empty-label="$t('costCenters.choose')" />
        </div>
        <div class="grid gap-2">
          <Label for="cc-default-from">{{ $t('costCenters.validFrom') }}</Label>
          <DateInput id="cc-default-from" v-model="newFrom" required />
        </div>
        <Button type="submit" variant="outline" :disabled="!newDefault"><Plus /> {{ $t('common.create') }}</Button>
      </form>
    </section>
  </div>

  <CostCenterDialog v-model:open="dialogOpen" :cost-center="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('costCenters.deleteConfirm', { name: deleting ? costCenterLabel(deleting) : '' })"
    @confirm="confirmDelete"
  />
</template>
