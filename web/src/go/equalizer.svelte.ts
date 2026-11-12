// Your own equalizer in web GO: the same twelve bands as the party's,
// kept on this device. The player's two audio elements are routed through
// Web Audio filters, but only once the equalizer is first turned on (and
// inside a tap, since browsers start sound only then); until then they
// play as they are. Once routed they stay routed until the page reloads,
// so turning it off sets every band flat.
import type { EqualizerSettings } from '../lib/types'

const KEY = 'djinn-go-equalizer'
/** The engine's bands and width (djinn-engine's equalizer.rs). */
const BAND_HZ = [32, 64, 125, 250, 500, 1000, 2000, 4000, 6000, 8000, 12000, 16000]
const Q = 1.4
const MAX_BAND_DB = 12

const flat = (): EqualizerSettings => ({ enabled: false, bands_db: Array(BAND_HZ.length).fill(0), output_gain_db: 0 })

function read(): EqualizerSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null') as EqualizerSettings | null
    if (
      saved &&
      Array.isArray(saved.bands_db) &&
      saved.bands_db.length === BAND_HZ.length &&
      saved.bands_db.every((g) => Number.isFinite(g) && Math.abs(g) <= MAX_BAND_DB) &&
      Number.isFinite(saved.output_gain_db)
    ) {
      return { enabled: !!saved.enabled, bands_db: saved.bands_db, output_gain_db: saved.output_gain_db }
    }
  } catch {
    // Private browsing or a bad value: start flat.
  }
  return flat()
}

let settings = read()
let elements: HTMLAudioElement[] = []
let context: AudioContext | null = null
let filters: BiquadFilterNode[] = []
let level: GainNode | null = null

/** Hands over the player's audio elements; the filters are built later. */
export function attachEqualizer(decks: HTMLAudioElement[]) {
  elements = decks
  // Any tap may build the filters (the setting may be on from last time)
  // or wake a sleeping audio context.
  const wake = () => {
    build()
    if (context && context.state !== 'running') void context.resume().catch(() => {})
  }
  document.addEventListener('pointerdown', wake, { passive: true, capture: true })
  document.addEventListener('keydown', wake, { capture: true })
}

/** Before playing: make sure the filters aren't asleep. */
export function wakeEqualizer() {
  if (context && context.state !== 'running') void context.resume().catch(() => {})
}

function build() {
  if (context || !settings.enabled || !elements.length || typeof AudioContext === 'undefined') return
  try {
    const ctx = new AudioContext()
    filters = BAND_HZ.map((hz) => {
      const filter = ctx.createBiquadFilter()
      filter.type = 'peaking'
      filter.frequency.value = Math.min(hz, ctx.sampleRate * 0.45)
      filter.Q.value = Q
      return filter
    })
    level = ctx.createGain()
    // Only peaks near clipping are held back, as on the party's speakers.
    const limiter = ctx.createDynamicsCompressor()
    limiter.threshold.value = -1
    limiter.knee.value = 0
    limiter.ratio.value = 20
    limiter.attack.value = 0.003
    limiter.release.value = 0.25
    filters.reduce<AudioNode>((from, to) => (from.connect(to), to), filters[0])
    filters[filters.length - 1].connect(level).connect(limiter).connect(ctx.destination)
    for (const element of elements) ctx.createMediaElementSource(element).connect(filters[0])
    context = ctx
    apply()
  } catch {
    context = null
  }
}

function apply() {
  if (!context || !level) return
  const at = context.currentTime
  filters.forEach((filter, i) => filter.gain.setTargetAtTime(settings.enabled ? settings.bands_db[i] : 0, at, 0.02))
  level.gain.setTargetAtTime(settings.enabled ? 10 ** (settings.output_gain_db / 20) : 1, at, 0.02)
}

/** For the settings panel. */
export async function loadEqualizer(): Promise<EqualizerSettings> {
  return { ...settings, bands_db: [...settings.bands_db] }
}

export async function saveEqualizer(next: EqualizerSettings) {
  settings = { ...next, bands_db: [...next.bands_db] }
  try {
    localStorage.setItem(KEY, JSON.stringify(settings))
  } catch {
    // Not kept, but it still applies until the page closes.
  }
  build()
  wakeEqualizer()
  apply()
}
