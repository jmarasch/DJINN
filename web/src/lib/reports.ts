// Why a listener says a song needs fixing, in the order they're offered.

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

export const reportReasons: { id: ReportReason; label: string }[] = [
  { id: 'wrong_info', label: 'The title, artist or album is wrong' },
  { id: 'wrong_cover', label: 'The cover is wrong' },
  { id: 'sounds_bad', label: 'It sounds bad, skips or cuts off' },
  { id: 'wrong_song', label: 'It plays a different song' },
  { id: 'other', label: 'Something else' },
]

/** Why a listener says a song's lyrics are bad. */
export const lyricsReasons: { id: ReportReason; label: string }[] = [
  { id: 'lyrics_wrong_song', label: "They're another song's lyrics" },
  { id: 'lyrics_out_of_sync', label: "They're out of time with the music" },
  { id: 'lyrics_garbled', label: 'They are garbled or in the wrong language' },
  { id: 'lyrics_missing', label: 'Parts are missing' },
  { id: 'lyrics_other', label: 'Something else' },
]

/** Short names for the Problems page. */
export const reportShort: Record<ReportReason, string> = {
  wrong_info: 'Wrong info',
  wrong_cover: 'Wrong cover',
  sounds_bad: 'Sounds bad',
  wrong_song: 'Wrong song',
  other: 'Other',
  lyrics_wrong_song: "Another song's lyrics",
  lyrics_out_of_sync: 'Out of sync',
  lyrics_garbled: 'Garbled or wrong language',
  lyrics_missing: 'Parts missing',
  lyrics_other: 'Other',
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
