import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type Absence, type TimesheetSubmission } from '@/bindings'
import { call } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'

/** Timesheets handed in for approval in the active organization workspace. */
export const useSubmissionsStore = defineStore('submissions', () => {
  const all = ref<TimesheetSubmission[]>([])
  /** Absence requests of other people waiting for a decision. */
  const absenceRequests = ref<Absence[]>([])
  const account = useAccountStore()
  /** What waits for the person's decision: others' timesheets and absence requests. */
  const waiting = computed(
    () => all.value.filter((s) => s.userId !== account.status.user?.id && s.status === 'submitted').length + absenceRequests.value.length,
  )

  async function load() {
    all.value = await call(commands.submissionsList())
    absenceRequests.value = await call(commands.absenceRequests())
  }
  async function submit(from: string, to: string, note: string) {
    await call(commands.submissionSubmit(from, to, note))
    await load()
  }
  async function decide(id: string, approve: boolean, note: string) {
    await call(commands.submissionDecide(id, approve, note))
    await load()
  }
  async function reopen(id: string, note: string) {
    await call(commands.submissionReopen(id, note))
    await load()
  }
  async function withdraw(id: string) {
    await call(commands.submissionWithdraw(id))
    await load()
  }
  return { all, absenceRequests, waiting, load, submit, decide, reopen, withdraw }
})
