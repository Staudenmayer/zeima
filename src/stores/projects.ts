import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type Project, type ProjectInput } from '@/bindings'
import { call } from '@/lib/ipc'

export const useProjectsStore = defineStore('projects', () => {
  /** All projects including archived ones. */
  const all = ref<Project[]>([])
  const loaded = ref(false)

  const byId = computed(() => new Map(all.value.map((p) => [p.id, p])))
  const active = computed(() => all.value.filter((p) => p.status !== 'archived'))

  async function load() {
    all.value = await call(commands.projectList(true))
    loaded.value = true
  }

  const create = (input: ProjectInput) => call(commands.projectCreate(input))
  const update = (id: string, input: ProjectInput) => call(commands.projectUpdate(id, input))
  const remove = (id: string) => call(commands.projectDelete(id))

  return { all, loaded, byId, active, load, create, update, remove }
})

export function projectToInput(p: Project): ProjectInput {
  return {
    name: p.name,
    alias: p.alias,
    color: p.color,
    parentId: p.parentId,
    client: p.client,
    budgetMinutes: p.budgetMinutes,
    budgetPeriod: p.budgetPeriod,
    budgetWarnPct: p.budgetWarnPct,
    billableDefault: p.billableDefault,
    status: p.status,
    costCenterId: p.costCenterId,
  }
}
