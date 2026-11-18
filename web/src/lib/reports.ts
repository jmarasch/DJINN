// Why a listener says a song needs fixing, in the order they're offered.
import { t } from './i18n.svelte'

export type ReportReason =
  | 'wrong_info'
  | 'wrong_cover'
  | 'sounds_bad'
  | 'wrong_song'
  | 'other'
  | 'lyrics_wrong_song'
  | 'lyrics_out_of_sync'
  | 'lyrics_garbled'
  | 'lyrics_missing'
  | 'lyrics_other'

/** A reason with its wording (`report.<id>`, shared with the Android app). */
const reason = (id: ReportReason) => ({
  id,
  get label() {
    return t(`report.${id}`)
  },
})

export const reportReasons: { id: ReportReason; label: string }[] = [
  reason('wrong_info'),
  reason('wrong_cover'),
  reason('sounds_bad'),
  reason('wrong_song'),
  reason('other'),
]

/** Why a listener says a song's lyrics are bad. */
export const lyricsReasons: { id: ReportReason; label: string }[] = [
  reason('lyrics_wrong_song'),
  reason('lyrics_out_of_sync'),
  reason('lyrics_garbled'),
  reason('lyrics_missing'),
  reason('lyrics_other'),
]

const REASONS = [...reportReasons, ...lyricsReasons].map((r) => r.id)

/** A reason's short name for the Problems page (`report.short_<id>`), or the reason itself. */
export const reportShort = (id: string) => (REASONS.includes(id as ReportReason) ? t(`report.short_${id}`) : id)

/** A song's reports from the Problems list: reason and note pairs. */
export function parseReports(detail: string): { reason: ReportReason; note: string }[] {
  return detail
    .split('\u001e')
    .filter(Boolean)
    .map((one) => {
      const [reason, note = ''] = one.split('\u001f')
      return { reason: reason as ReportReason, note }
    })
}
