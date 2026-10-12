import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type CostCenter, type CostCenterAssignment, type CostCenterInput } from '@/bindings'
import { call } from '@/lib/ipc'
import { toDateInput } from '@/lib/time'

export const useCostCentersStore = defineStore('costCenters', () => {
  /** All cost centers, including inactive ones. */
  const all = ref<CostCenter[]>([])
  /** The user's default cost centers, newest first. */
  const assignments = ref<CostCenterAssignment[]>([])

  const byId = computed(() => new Map(all.value.map((c) => [c.id, c])))

  /** Active and valid on `date` (`YYYY-MM-DD`, default today). */
  function usable(date = toDateInput(Date.now())) {
    return all.value.filter(
      (c) => c.active && (!c.validFrom || c.validFrom <= date) && (!c.validTo || date <= c.validTo),
    )
  }

  async function load() {
    const [list, defaults] = await Promise.all([
      call(commands.costCenterList()),
      call(commands.costCenterAssignmentList()),
    ])
    all.value = list
    assignments.value = defaults
  }

  const save = (id: string | null, input: CostCenterInput) => call(commands.costCenterSave(id, input))
  const remove = (id: string) => call(commands.costCenterDelete(id))

  return { all, assignments, byId, usable, load, save, remove }
})

export function costCenterLabel(c: CostCenter | null | undefined): string {
  return c ? `${c.code} · ${c.name}` : ''
}
