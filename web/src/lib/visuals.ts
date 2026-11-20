// Visualizers for node screens: every style, the settings Main keeps for
// them (`/api/visuals`, a node's own in its screen settings), a made-up song
// for previews, and thumbnails drawn from it.

import { GlVisual } from './nodeGlVisual'
import { isGlStyle, ShapeVisual, Smoother, type ShapeStyle, type SoundFrame, type VisualStyle } from './nodeVisual'
import { t } from './i18n.svelte'

export interface Look {
  /** How strongly it reacts, 0.25 to 3. */
  intensity: number
  colors: 'default' | 'rainbow'
}

export interface Visuals {
  /** The styles in the rotation, in order. */
  styles: VisualStyle[]
  /** The next style with each song, every `every_secs`, or never. */
  change: 'song' | 'time' | 'never'
  every_secs: number
  looks: Partial<Record<VisualStyle, Look>>
}

export const DEFAULT_LOOK: Look = { intensity: 1, colors: 'default' }

export const DEFAULT_VISUALS: Visuals = {
  styles: ['spectrogram', 'bars', 'wave', 'ring', 'plasma', 'tunnel'],
  change: 'song',
  every_secs: 60,
  looks: {},
}

export interface StyleInfo {
  style: VisualStyle
  name: string
  about: string
  /** Drawn on the graphics chip: smooth on most boxes, too much for some. */
  chip: boolean
}

/** A style's entry; its name (`visual.<style>`, shared with the Android app)
 * and description (`visual.about_<style>`) follow the chosen language. */
const info = (style: VisualStyle, chip: boolean): StyleInfo => ({
  style,
  chip,
  get name() {
    return t(`visual.${style}`)
  },
  get about() {
    return t(`visual.about_${style}`)
  },
})

export const STYLES: StyleInfo[] = [
  info('bars', false),
  info('ring', false),
  info('mirror', false),
  info('wave', false),
  info('spectrogram', false),
  info('scope', false),
  info('mountain', false),
  info('vu', false),
  info('circle', false),
  info('particles', false),
  info('terrain', false),
  info('flyover', false),
  info('warp', true),
  info('tunnel', true),
  info('plasma', true),
  info('kaleidoscope', true),
  info('nebula', true),
  info('stars', true),
]

export const styleInfo = (style: string) => STYLES.find((s) => s.style === style)

export const lookOf = (visuals: Visuals, style: VisualStyle): Look => visuals.looks[style] ?? DEFAULT_LOOK

/** A made-up song for previews: a kick twice a second, a bass line, a
 * melody wandering about and some hiss, as the node would measure it. */
export function fakeSound(seconds: number): SoundFrame {
  const beat = seconds % 0.5
  const kick = Math.exp(-beat * 14)
  const notes = [220, 262, 330, 392, 349, 294]
  const melody = notes[Math.floor(seconds * 2) % notes.length]
  const bassNote = [55, 55, 73, 65][Math.floor(seconds / 2) % 4]
  // Loudness in dB-ish terms (0 to 1) at a frequency.
  const at = (hz: number) => {
    const near = (center: number, width: number) => Math.exp(-(Math.log2(hz / center) ** 2) / width)
    const level =
      0.85 * kick * near(60, 0.4) +
      0.6 * near(bassNote, 0.05) +
      0.7 * near(melody, 0.01) +
      0.45 * near(melody * 2, 0.01) +
      0.3 * near(melody * 3, 0.01) +
      0.25 * near(6000, 3) * (0.6 + 0.4 * Math.sin(seconds * 31)) +
      0.12 * Math.random()
    return Math.min(255, Math.round(Math.min(1, level) * 255))
  }
  const rows = (count: number, low: number) =>
    Array.from({ length: count }, (_, i) => at(low * (16000 / low) ** ((i + 0.5) / count)))
  const wave = (side: number) =>
    Array.from({ length: 128 }, (_, i) => {
      const t = seconds + (i / 128) * 0.021
      const s =
        0.45 * kick * Math.sin(2 * Math.PI * 60 * t) +
        0.25 * Math.sin(2 * Math.PI * melody * t + side) +
        0.15 * Math.sin(2 * Math.PI * bassNote * t)
      return Math.round((Math.max(-1, Math.min(1, s)) * 0.5 + 0.5) * 255)
    })
  const loud = Math.round((0.55 + 0.35 * kick) * 255)
  return { b: rows(24, 40), f: rows(96, 30), l: wave(0), r: wave(0.6), v: [loud, Math.round(loud * 0.92)] }
}

/** Draws a style from the made-up song, frame by frame. */
export class Preview {
  private sound = new Smoother()
  private shape: ShapeVisual | null = null
  private chip: GlVisual | null = null

  constructor(
    private canvas: HTMLCanvasElement,
    style: VisualStyle,
    look: Look,
  ) {
    this.sound.gain = look.intensity
    if (isGlStyle(style)) {
      this.chip = new GlVisual(canvas, style)
      this.chip.rainbow = look.colors === 'rainbow'
    } else {
      const ctx = canvas.getContext('2d')
      if (!ctx) throw new Error('no 2D canvas')
      this.shape = new ShapeVisual(ctx, style as ShapeStyle)
      this.shape.rainbow = look.colors === 'rainbow'
    }
  }

  /** Moves on to `ms` (in the made-up song and on screen). */
  draw(ms: number) {
    this.sound.step(fakeSound(ms / 1000), ms)
    if (this.chip) this.chip.draw(this.sound, ms)
    else this.shape?.draw(this.sound, ms)
  }

  /** Lets go of the graphics chip. */
  close() {
    this.chip?.close()
  }
}

const thumbs = new Map<string, Promise<string>>()

/** A small picture of a style, a few seconds into the made-up song. */
export function thumbnail(style: VisualStyle): Promise<string> {
  let made = thumbs.get(style)
  if (!made) {
    made = new Promise<string>((resolve) => {
      // One at a time, between frames, so the page stays responsive.
      setTimeout(() => {
        try {
          const canvas = document.createElement('canvas')
          canvas.width = 320
          canvas.height = 180
          const preview = new Preview(canvas, style, DEFAULT_LOOK)
          // Ten seconds, so the spectrogram fills across.
          for (let ms = 0; ms <= 10_000; ms += 33) preview.draw(ms)
          const url = canvas.toDataURL('image/jpeg', 0.8)
          preview.close()
          resolve(url)
        } catch {
          resolve('')
        }
      }, thumbs.size * 30)
    })
    thumbs.set(style, made)
  }
  return made
}
