import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type Absence } from '@/bindings'
import { call } from '@/lib/ipc'

/** Absence requests in the active organization workspace that wait for the person's decision. */
export const useApprovalsStore = defineStore('approvals', () => {
  const absenceRequests = ref<Absence[]>([])
  const waiting = computed(() => absenceRequests.value.length)

  async function load() {
    absenceRequests.value = await call(commands.absenceRequests())
  }
  return { absenceRequests, waiting, load }
})
