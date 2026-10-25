export function duration(ms: number | null | undefined): string {
  if (ms == null || ms < 0) return ''
  const total = Math.floor(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${s.toString().padStart(2, '0')}`
}

/** "Artist · Album" with whatever parts exist. */
export function byline(artist: string | null, album?: string | null): string {
  return [artist, album].filter(Boolean).join(' · ')
}
