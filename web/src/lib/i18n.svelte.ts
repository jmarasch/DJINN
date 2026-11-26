// Translations for the screens guests use. Each language is a JSON file in
// src/i18n/ (en.json is the reference); a missing key falls back to English,
// then to the key itself. The language is the one picked on this device
// (Your account), else the house's default (Admin > System,
// `/api/language`), else the phone's own (navigator languages).
//
// t('queue.title') or t('queue.count', { n: 3 }): placeholders are {name};
// a key whose English entry is an object { one, other } picks the plural
// form for {n} the way the language counts.
//
// The files are built in, and DJINN may also have them in a folder beside
// the program (`/api/language/files`, only when they belong to this
// version). Those are laid over the built-in ones, so a fixed translation or
// a new language shows up without a new build.

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
export const languages = $state<{ code: string; name: string }[]>(listed())

function listed() {
  return Object.keys(catalogs)
    .sort()
    .map((code) => ({ code, name: String(catalogs[code]['language.name'] ?? code) }))
}

/** Bumped when the files beside the program arrive, so text redraws. */
const revision = $state({ n: 0 })

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

/** The language this device picked for itself on Your Account ('' when it
 * follows the house). */
export function ownPick(): string {
  const chosen = stored()
  return chosen && catalogs[chosen] ? chosen : ''
}

/** The house's default changed (the live `language` event, or a fresh ask):
 * switches to it unless this device picked its own. */
export function houseLanguageIs(code: string) {
  house.code = code ?? ''
  if (!stored()) setLanguage('')
}

/** Asks the server for the house's default and switches to it unless this
 * device picked its own. */
export async function loadHouseLanguage() {
  try {
    const res = await fetch('/api/language')
    if (!res.ok) return
    houseLanguageIs(((await res.json()) as { default: string }).default ?? '')
  } catch {
    // Offline: the phone's own language stays.
  }
}

/** Lays the language files beside the program over the built-in ones. */
export async function loadLanguageFiles() {
  try {
    const res = await fetch('/api/language/files')
    if (!res.ok) return
    const files = (await res.json()) as Record<string, Catalog>
    if (!Object.keys(files).length) return
    for (const [code, catalog] of Object.entries(files)) catalogs[code] = { ...catalogs[code], ...catalog }
    languages.splice(0, languages.length, ...listed())
    locale.code = pick()
    document.documentElement.lang = locale.code
    revision.n++
  } catch {
    // The built-in files stay.
  }
}

if (typeof fetch !== 'undefined' && typeof location !== 'undefined') {
  void loadHouseLanguage()
  void loadLanguageFiles()
  // A phone that kept the app open in the background (web GO, the TV and
  // lyrics pages have no live feed of it) asks again when it comes back.
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') void loadHouseLanguage()
  })
}

function lookup(code: string, key: string): string | Record<string, string> | undefined {
  return catalogs[code]?.[key]
}

/** The text for `key` in the current language. */
export function t(key: string, params: Record<string, string | number> = {}): string {
  void revision.n
  let entry = lookup(locale.code, key) ?? lookup('en', key) ?? key
  if (typeof entry === 'object') {
    const n = countOf(params.n ?? 0)
    const form = new Intl.PluralRules(locale.code).select(n)
    entry = entry[form] ?? entry.other ?? Object.values(entry)[0] ?? key
  }
  return entry.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(name === 'n' && typeof params.n === 'number' ? params.n.toLocaleString(locale.code) : params[name]) : match,
  )
}

/** The number in `n` for choosing a plural: a number, or one already written
    out for showing ("1,234", "1.234"), whose separators are dropped. */
function countOf(value: string | number): number {
  if (typeof value === 'number') return value
  const plain = Number(value.replace(/[^\d-]/g, ''))
  return Number.isNaN(plain) ? 0 : plain
}
