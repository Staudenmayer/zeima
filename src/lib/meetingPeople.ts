import type { MeetingOccurrence } from '@/bindings'

export type Answer = 'organizer' | 'accepted' | 'tentative' | 'declined' | 'waiting'

/** Ring colour of each answer around a person's picture. */
export const ANSWER_COLORS: Record<Answer, string> = {
  organizer: '#3b82f6',
  accepted: '#22c55e',
  tentative: '#f59e0b',
  declined: '#ef4444',
  waiting: '#9ca3af',
}

/** Everyone who takes part in one date of a meeting — the organizer first — with what they answered. */
export function participantsOf(o: MeetingOccurrence): { userId: string; answer: Answer }[] {
  const answerOf = (id: string): Answer => o.responses.find((r) => r.userId === id)?.status ?? 'waiting'
  return [{ userId: o.meeting.userId, answer: 'organizer' }, ...o.meeting.attendeeIds.map((id) => ({ userId: id, answer: answerOf(id) }))]
}
