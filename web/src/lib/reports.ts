// Why a listener says a song needs fixing, in the order they're offered.

export type ReportReason = 'wrong_info' | 'wrong_cover' | 'sounds_bad' | 'wrong_song' | 'other'

export const reportReasons: { id: ReportReason; label: string }[] = [
  { id: 'wrong_info', label: 'The title, artist or album is wrong' },
  { id: 'wrong_cover', label: 'The cover is wrong' },
  { id: 'sounds_bad', label: 'It sounds bad, skips or cuts off' },
  { id: 'wrong_song', label: 'It plays a different song' },
  { id: 'other', label: 'Something else' },
]

/** Short names for the Problems page. */
export const reportShort: Record<ReportReason, string> = {
  wrong_info: 'Wrong info',
  wrong_cover: 'Wrong cover',
  sounds_bad: 'Sounds bad',
  wrong_song: 'Wrong song',
  other: 'Other',
}

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
