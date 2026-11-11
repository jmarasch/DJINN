// The visualizer a node draws on its own screen, from the levels its speaker
// plays (`/api/node/levels`). The shapes and colors follow DJINN GO for
// Android's Bars, Ring and Mirror; the shader styles are in `nodeGlVisual.ts`.

export type ShapeStyle = 'bars' | 'ring' | 'mirror'
export type GlStyle = 'warp' | 'tunnel' | 'plasma' | 'kaleidoscope' | 'nebula' | 'stars'
export type VisualStyle = ShapeStyle | GlStyle

export const GL_STYLES: GlStyle[] = ['warp', 'tunnel', 'plasma', 'kaleidoscope', 'nebula', 'stars']
export const isGlStyle = (style: string): style is GlStyle => (GL_STYLES as string[]).includes(style)

/** Bands below this are bass, below `MID_TOP` mid, the rest treble (the node
 * sends 24 bands from 40 Hz to 16 kHz). */
const BASS_TOP = 8
const MID_TOP = 16
/** A beat is bass this far over what it has been lately. */
const BEAT_RISE = 0.08
const BEAT_FLOOR = 0.35
const BEAT_GAP_MS = 180
const BASS_FOLLOW = 0.08
const BEAT_FALL_PER_SECOND = 4
/** How slowly the slow parts follow, and the most one step moves them. */
const SLOW_SECONDS = 0.25
const MAX_STEP_SECONDS = 0.1

/** How fast a band falls back, in full heights a second. */
const FALL_PER_SECOND = 1.6
const LOW = [180, 140, 255]
const HIGH = [255, 255, 255]
const GAP = 0.2

const average = (list: number[]) => (list.length ? list.reduce((a, b) => a + b, 0) / list.length : 0)

/** Follows the levels smoothly (up at once, down gently), with bass, mid,
 * treble, beats, and the slow motion the shader styles move by, as
 * DJINN GO for Android's `SoundReader` and `VisualMotion` do. */
export class Smoother {
  bands: number[] = []
  bass = 0
  mid = 0
  treble = 0
  /** 1 on a beat, falling away to 0. */
  beat = 0
  slowBass = 0
  slowMid = 0
  slowTreble = 0
  tunnel = 0
  stars = 0
  private last = 0
  private lastLevels: number[] | null = null
  private bassAverage = 0
  private lastBeat = 0

  step(levels: number[] | null, now: number) {
    const elapsed = this.last ? Math.min(0.2, (now - this.last) / 1000) : 0
    this.last = now
    const fall = FALL_PER_SECOND * elapsed
    const count = levels?.length ?? this.bands.length
    for (let i = 0; i < count; i++) {
      const level = levels ? levels[i] / 255 : 0
      const was = this.bands[i] ?? 0
      this.bands[i] = level > was ? level : Math.max(level, was - fall)
    }
    const follow = (was: number, level: number) => (level > was ? level : Math.max(level, was - fall))
    const raw = (levels ?? []).map((l) => l / 255)
    this.bass = follow(this.bass, average(raw.slice(0, BASS_TOP)))
    this.mid = follow(this.mid, average(raw.slice(BASS_TOP, MID_TOP)))
    this.treble = follow(this.treble, average(raw.slice(MID_TOP)))
    // Beats, from each new measurement.
    this.beat = Math.max(0, this.beat - BEAT_FALL_PER_SECOND * elapsed)
    if (levels && levels !== this.lastLevels) {
      this.lastLevels = levels
      const bass = average(raw.slice(0, BASS_TOP))
      if (bass > this.bassAverage + BEAT_RISE && bass > BEAT_FLOOR && now - this.lastBeat > BEAT_GAP_MS) {
        this.beat = 1
        this.lastBeat = now
      }
      this.bassAverage += (bass - this.bassAverage) * BASS_FOLLOW
    }
    const by = Math.min(elapsed, MAX_STEP_SECONDS)
    const slow = 1 - Math.exp(-by / SLOW_SECONDS)
    this.slowBass += (this.bass - this.slowBass) * slow
    this.slowMid += (this.mid - this.slowMid) * slow
    this.slowTreble += (this.treble - this.slowTreble) * slow
    this.tunnel = (this.tunnel + by * (0.35 + 0.5 * this.slowBass)) % 3600
    // Only its fraction is used, so it can wrap without a jump.
    this.stars = (this.stars + by * (0.05 + 0.03 * this.slowBass)) % 1
  }
}

function paint(level: number): string {
  const t = Math.min(1, Math.max(0, level * level))
  const [r, g, b] = LOW.map((low, i) => Math.round(low + (HIGH[i] - low) * t))
  return `rgb(${r} ${g} ${b})`
}

/** Draws one frame of `style` filling the canvas. */
export function draw(ctx: CanvasRenderingContext2D, style: ShapeStyle, sound: Smoother) {
  const { width, height } = ctx.canvas
  ctx.clearRect(0, 0, width, height)
  // Music keeps most bands well up the scale; squaring gives them room to move.
  const bands = sound.bands.map((b) => b * b)
  const count = bands.length
  if (!count) return
  if (style === 'ring') {
    const cx = width / 2
    const cy = height / 2
    const inner = Math.min(width, height) * (0.22 + 0.04 * sound.beat)
    const longest = Math.min(width, height) * 0.26
    // Mirrored, so the two sides meet without a jump.
    const spokes = count * 2
    ctx.lineCap = 'round'
    ctx.lineWidth = ((2 * Math.PI * inner) / spokes) * (1 - GAP)
    for (let i = 0; i < spokes; i++) {
      const band = i < count ? i : spokes - 1 - i
      const angle = (2 * Math.PI * i) / spokes - Math.PI / 2
      const length = inner + 2 + longest * bands[band]
      ctx.strokeStyle = paint(bands[band])
      ctx.beginPath()
      ctx.moveTo(cx + Math.cos(angle) * inner, cy + Math.sin(angle) * inner)
      ctx.lineTo(cx + Math.cos(angle) * length, cy + Math.sin(angle) * length)
      ctx.stroke()
    }
    return
  }
  const gap = (width / count) * GAP
  const barWidth = width / count - gap
  for (let i = 0; i < count; i++) {
    ctx.fillStyle = paint(bands[i])
    const x = i * (barWidth + gap) + gap / 2
    if (style === 'mirror') {
      const h = Math.max(barWidth / 2, (height / 2) * bands[i])
      roundRect(ctx, x, height / 2 - h, barWidth, h * 2, barWidth / 2)
    } else {
      const h = Math.max(barWidth, height * 0.85 * bands[i])
      roundRect(ctx, x, height - h, barWidth, h, barWidth / 2)
    }
  }
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath()
  ctx.roundRect(x, y, w, h, Math.min(r, h / 2))
  ctx.fill()
}
