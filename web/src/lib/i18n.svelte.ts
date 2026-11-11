// Translations for the screens guests use. Each language is a JSON file in
// src/i18n/ (en.json is the reference); a missing key falls back to English,
// then to the key itself. The language is the one picked on this device
// (Your account), else the house's default (Admin > System,
// `/api/language`), else the phone's own (navigator languages).
//
// t('queue.title') or t('queue.count', { n: 3 }): placeholders are {name};
// a key whose English entry is an object { one, other } picks the plural
// form for {n} the way the language counts.

const files = import.meta.glob('../i18n/*.json', { eager: true, import: 'default' }) as Record<
  string,
  Record<string, string | Record<string, string>>
>

type Catalog = Record<string, string | Record<string, string>>

const catalogs: Record<string, Catalog> = {}
for (const [path, catalog] of Object.entries(files)) {
  const code = path.split('/').pop()?.replace('.json', '')
  if (code) catalogs[code] = catalog
}

/** Languages there are translations for, with their own names. */
export const languages: { code: string; name: string }[] = Object.keys(catalogs)
  .sort()
  .map((code) => ({ code, name: String(catalogs[code]['language.name'] ?? code) }))

function stored(): string | null {
  try {
    return localStorage.getItem('djinn-language')
  } catch {
    return null
  }
}

/** The house's default language, once the server has said ('' for each
 * device's own). */
export const house = $state({ code: '' })

/** The best language for this browser: its own pick, the house's default,
 * or the first it asks for that we have. */
function pick(): string {
  const chosen = stored()
  if (chosen && catalogs[chosen]) return chosen
  if (house.code && catalogs[house.code]) return house.code
  const asked = typeof navigator === 'undefined' ? [] : (navigator.languages ?? [navigator.language])
  for (const tag of asked) {
    const lower = tag.toLowerCase()
    if (catalogs[lower]) return lower
    const base = lower.split('-')[0]
    if (catalogs[base]) return base
  }
  return 'en'
}

export const locale = $state({ code: pick() })

/** Picks a language (empty for the phone's own). */
export function setLanguage(code: string) {
  try {
    if (code) localStorage.setItem('djinn-language', code)
    else localStorage.removeItem('djinn-language')
  } catch {
    // Not remembered.
  }
  locale.code = code && catalogs[code] ? code : pick()
  document.documentElement.lang = locale.code
}

if (typeof document !== 'undefined') document.documentElement.lang = locale.code

/** Asks the server for the house's default and switches to it unless this
 * device picked its own. */
export async function loadHouseLanguage() {
  try {
    const res = await fetch('/api/language')
    if (!res.ok) return
    house.code = ((await res.json()) as { default: string }).default ?? ''
    if (!stored()) setLanguage('')
  } catch {
    // Offline: the phone's own language stays.
  }
}

if (typeof fetch !== 'undefined' && typeof location !== 'undefined') void loadHouseLanguage()

function lookup(code: string, key: string): string | Record<string, string> | undefined {
  return catalogs[code]?.[key]
}

/** The text for `key` in the current language. */
export function t(key: string, params: Record<string, string | number> = {}): string {
  let entry = lookup(locale.code, key) ?? lookup('en', key) ?? key
  if (typeof entry === 'object') {
    const n = Number(params.n ?? 0)
    const form = new Intl.PluralRules(locale.code).select(n)
    entry = entry[form] ?? entry.other ?? Object.values(entry)[0] ?? key
  }
  return entry.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(name === 'n' ? Number(params[name]).toLocaleString(locale.code) : params[name]) : match,
  )
}
