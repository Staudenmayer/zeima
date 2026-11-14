/** When someone is busy, as the server tells: the time, never what for. */
export interface Busy {
  from: number
  to: number
  kind: 'meeting' | 'absence'
}

/** Who and when: what the timesheet shows of another person. */
export interface PersonDay {
  id: string
  name: string
  entries: import('@/bindings').TimeEntry[]
  busy: Busy[]
}
