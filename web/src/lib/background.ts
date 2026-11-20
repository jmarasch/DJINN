// Wording for heavy background work (sound analysis, AI listening) that
// waits for its turn.
import type { Held } from './types'
import { locale, t } from './i18n.svelte'

/** An hour of the day the way this device writes it ("1 AM", "13"). */
export const clock = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString(locale.code, { hour: 'numeric' })

/** Why the work waits. */
export function heldText(held: Held): string {
  return held.reason === 'music'
    ? t('bg.paused_music')
    : t('bg.waiting_hours', { from: clock(held.from), until: clock(held.until) })
}
