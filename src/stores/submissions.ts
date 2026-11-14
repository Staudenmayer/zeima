import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, type TimesheetSubmission } from '@/bindings'
import { call } from '@/lib/ipc'

/** Timesheets handed in for approval in the active organization workspace. */
export const useSubmissionsStore = defineStore('submissions', () => {
  const all = ref<TimesheetSubmission[]>([])

  async function load() {
    all.value = await call(commands.submissionsList())
  }
  async function submit(from: string, to: string, note: string) {
    await call(commands.submissionSubmit(from, to, note))
    await load()
  }
  async function decide(id: string, approve: boolean, note: string) {
    await call(commands.submissionDecide(id, approve, note))
    await load()
  }
  async function withdraw(id: string) {
    await call(commands.submissionWithdraw(id))
    await load()
  }
  return { all, load, submit, decide, withdraw }
})
