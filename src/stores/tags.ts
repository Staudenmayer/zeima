import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type Tag, type TagInput } from '@/bindings'
import { call } from '@/lib/ipc'

export const useTagsStore = defineStore('tags', () => {
  const all = ref<Tag[]>([])

  const byId = computed(() => new Map(all.value.map((t) => [t.id, t])))

  /** Global tags plus the tags scoped to `projectId`. */
  function usableFor(projectId: string | null): Tag[] {
    return all.value.filter((t) => t.projectId === null || t.projectId === projectId)
  }

  async function load() {
    all.value = await call(commands.tagList())
  }

  const create = (input: TagInput) => call(commands.tagCreate(input))
  const update = (id: string, input: TagInput) => call(commands.tagUpdate(id, input))
  const remove = (id: string) => call(commands.tagDelete(id))

  return { all, byId, usableFor, load, create, update, remove }
})
