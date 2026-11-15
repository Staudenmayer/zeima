/** How a meeting looks in the calendar for the person's answer: solid once answered (accepted green, maybe amber, declined red and dimmed), dashed only while it is not answered. */
export type MeetingState = 'accepted' | 'tentative' | 'declined' | 'none'

const COLORS: Record<MeetingState, string> = {
  accepted: '#16a34a',
  tentative: '#f59e0b',
  declined: '#dc2626',
  none: 'var(--muted-foreground)',
}

export function meetingLook(state: MeetingState) {
  const color = COLORS[state]
  return {
    borderColor: color,
    borderStyle: state === 'none' ? 'dashed' : 'solid',
    borderWidth: state === 'none' ? '1px' : '2px',
    backgroundColor: `color-mix(in srgb, ${color} ${state === 'none' ? 6 : state === 'declined' ? 12 : 22}%, transparent)`,
    opacity: state === 'declined' ? 0.6 : 1,
  }
}
