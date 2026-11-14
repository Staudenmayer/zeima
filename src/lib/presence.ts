import type { PresenceItem } from '@/stores/org'

/** The colour of each presence state; a person who is not reporting is offline (grey). */
export const PRESENCE_COLORS: Record<PresenceItem['state'] | 'offline', string> = {
  online: '#22c55e',
  in_meeting: '#ef4444',
  traveling: '#8b5cf6',
  away: '#f59e0b',
  offline: '#9ca3af',
}
