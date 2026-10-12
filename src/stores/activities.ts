import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type Activity, type ActivityInput } from '@/bindings'
import { call } from '@/lib/ipc'

export const useActivitiesStore = defineStore('activities', () => {
  /** All activities including archived ones, by name. */
  const all = ref<Activity[]>([])

  const byId = computed(() => new Map(all.value.map((a) => [a.id, a])))
  const active = computed(() => all.value.filter((a) => !a.archived))

  async function load() {
    all.value = await call(commands.activityList(true))
  }

  const create = (input: ActivityInput) => call(commands.activityCreate(input))
  const update = (id: string, input: ActivityInput) => call(commands.activityUpdate(id, input))
  const remove = (id: string) => call(commands.activityDelete(id))

  return { all, byId, active, load, create, update, remove }
})
