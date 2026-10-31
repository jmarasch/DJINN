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

/** A name as sorted: a leading "The", "A", or "An" doesn't count (the
 * server sorts the same way). */
export function sortName(name: string): string {
  return name.replace(/^(the|an|a)\s+/i, '').toLocaleLowerCase()
}

export const byName = <T>(key: (item: T) => string) => (a: T, b: T) =>
  sortName(key(a)).localeCompare(sortName(key(b)))

/** "Firefox on Windows" from a user agent, or the raw text. */
export function deviceName(agent: string | null): string {
  if (!agent) return 'Unknown device'
  const browser = /Edg\//.test(agent)
    ? 'Edge'
    : /Firefox\//.test(agent)
      ? 'Firefox'
      : /Chrome\//.test(agent)
        ? 'Chrome'
        : /Safari\//.test(agent)
          ? 'Safari'
          : null
  const system = /iPhone|iPad/.test(agent)
    ? 'iPhone or iPad'
    : /Android/.test(agent)
      ? 'Android'
      : /Windows/.test(agent)
        ? 'Windows'
        : /Mac OS X/.test(agent)
          ? 'Mac'
          : /Linux/.test(agent)
            ? 'Linux'
            : null
  if (browser && system) return `${browser} on ${system}`
  return browser ?? system ?? agent.slice(0, 60)
}
