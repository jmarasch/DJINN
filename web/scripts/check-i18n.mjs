// Checks the translations (src/i18n): every key the screens use is in
// English, every language has every English key, and each translation has
// the same {placeholders}. Part of `npm run check`.
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const dir = 'src/i18n'
const catalogs = Object.fromEntries(
  readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .map((f) => [f.replace('.json', ''), JSON.parse(readFileSync(join(dir, f), 'utf8'))]),
)
const en = catalogs.en
const problems = []
const places = (v) => [...JSON.stringify(v).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',')

for (const [code, catalog] of Object.entries(catalogs)) {
  for (const key of Object.keys(en)) {
    if (!(key in catalog)) problems.push(`${code}: missing ${key}`)
    else if (places(catalog[key]) !== places(en[key])) problems.push(`${code}: ${key} has different {placeholders}`)
  }
  for (const key of Object.keys(catalog)) if (!(key in en)) problems.push(`${code}: ${key} isn't in en.json`)
}

function files(path) {
  return statSync(path).isDirectory()
    ? readdirSync(path).flatMap((f) => files(join(path, f)))
    : /\.(svelte|ts)$/.test(path)
      ? [path]
      : []
}
for (const file of files('src')) {
  for (const m of readFileSync(file, 'utf8').matchAll(/\bt\(['`]([a-z_]+\.[a-z_.]+)['`]/g)) {
    if (!(m[1] in en) && file.indexOf('i18n.svelte.ts') < 0) problems.push(`${file}: ${m[1]} isn't in en.json`)
  }
}

if (problems.length) {
  console.error(problems.join('\n'))
  process.exit(1)
}
console.log(`translations ok: ${Object.keys(catalogs).length} languages, ${Object.keys(en).length} keys`)
