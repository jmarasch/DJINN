// Searching the user guide: each page is split into its sections (by
// heading), and a search finds the sections holding every word typed,
// headings counting most.

import { slug } from './markdown'

export interface GuideSection {
  page: string
  /** The page's title (its first heading). */
  pageTitle: string
  /** The section's heading, and its anchor ('' for the page's top). */
  heading: string
  anchor: string
  /** The section's words as plain text. */
  text: string
}

export interface GuideHit {
  section: GuideSection
  /** A piece of the text around the first match. */
  snippet: string
  score: number
}

/** Markdown down to the words a reader sees. */
function plain(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/\*\*|`/g, '')
    .replace(/^\s*>\s?/gm, ' ')
    .replace(/[|#]/g, ' ')
    .replace(/^\s*([-*]|\d+\.)\s+/gm, ' ')
    .replace(/-{3,}/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

/** A page's sections, split at its headings. */
export function sections(page: string, markdown: string): GuideSection[] {
  const out: GuideSection[] = []
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n')
  let pageTitle = page.replace(/\.md$/, '')
  let heading = ''
  let body: string[] = []
  let inCode = false
  const flush = () => {
    const text = plain(body.join('\n'))
    if (text || heading) out.push({ page, pageTitle, heading, anchor: heading ? slug(heading) : '', text })
    body = []
  }
  for (const line of lines) {
    if (/^\s*```/.test(line)) inCode = !inCode
    const m = !inCode && line.match(/^(#{1,6})\s+(.*)$/)
    if (m) {
      if (m[1].length === 1 && !out.length && !heading) {
        pageTitle = m[2].trim()
        continue
      }
      flush()
      heading = m[2].trim()
    } else body.push(line)
  }
  flush()
  return out
}

const fold = (s: string) =>
  s
    .toLowerCase()
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')

/** The words to look for, in lowercase. */
export function words(query: string): string[] {
  return fold(query)
    .split(/[^\p{L}\p{N}]+/u)
    .filter((w) => w.length > 1)
}

/** Sections holding every word of `query`, best first; when none holds
 * them all, the ones holding the most. */
export function search(all: GuideSection[], query: string, limit = 40): GuideHit[] {
  const wanted = words(query)
  if (!wanted.length) return []
  const phrase = wanted.join(' ')
  const hits: (GuideHit & { found: number })[] = []
  for (const section of all) {
    const heading = fold(section.heading + ' ' + section.pageTitle)
    const text = fold(section.text)
    let score = 0
    let found = 0
    let first = -1
    for (const w of wanted) {
      const inHeading = heading.includes(w)
      const at = text.indexOf(w)
      if (!inHeading && at < 0) continue
      found++
      if (first < 0 && at >= 0) first = at
      score += (inHeading ? 10 : 0) + Math.min(text.split(w).length - 1, 5)
    }
    if (!found) continue
    if (wanted.length > 1) {
      if (heading.includes(phrase)) score += 25
      const at = text.indexOf(phrase)
      if (at >= 0) {
        score += 15
        first = at
      }
    }
    const start = Math.max(0, section.text.lastIndexOf(' ', Math.max(0, first - 60)) + 1)
    let snippet = section.text.slice(start, start + 180)
    if (start > 0) snippet = '... ' + snippet
    if (start + 180 < section.text.length) snippet += ' ...'
    hits.push({ section, snippet, score, found })
  }
  const best = Math.max(0, ...hits.map((h) => h.found))
  return hits
    .filter((h) => h.found === best)
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map(({ section, snippet, score }) => ({ section, snippet, score }))
}

/** `text` split into plain pieces and the pieces matching `wanted`, for
 * marking them without building HTML. */
export function marked(text: string, wanted: string[]): { text: string; hit: boolean }[] {
  if (!wanted.length) return [{ text, hit: false }]
  const folded = fold(text)
  const out: { text: string; hit: boolean }[] = []
  let i = 0
  while (i < text.length) {
    let next = -1
    let length = 0
    for (const w of wanted) {
      const at = folded.indexOf(w, i)
      if (at >= 0 && (next < 0 || at < next)) {
        next = at
        length = w.length
      }
    }
    if (next < 0) break
    if (next > i) out.push({ text: text.slice(i, next), hit: false })
    out.push({ text: text.slice(next, next + length), hit: true })
    i = next + length
  }
  if (i < text.length) out.push({ text: text.slice(i), hit: false })
  return out
}
