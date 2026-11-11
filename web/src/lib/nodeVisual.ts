// The visualizer a node draws on its own screen, from what its speaker plays
// (`/api/node/levels`). Bars, Ring, Mirror and Wave follow DJINN GO for
// Android's shapes; Spectrogram, Scope, Mountain, VU, Circle and Particles
// are the node's own. The shader styles are in `nodeGlVisual.ts`.

export type ShapeStyle =
  | 'bars'
  | 'ring'
  | 'mirror'
  | 'wave'
  | 'spectrogram'
  | 'scope'
  | 'mountain'
  | 'vu'
  | 'circle'
  | 'particles'
export type GlStyle = 'warp' | 'tunnel' | 'plasma' | 'kaleidoscope' | 'nebula' | 'stars'
export type VisualStyle = ShapeStyle | GlStyle

export const GL_STYLES: GlStyle[] = ['warp', 'tunnel', 'plasma', 'kaleidoscope', 'nebula', 'stars']
export const isGlStyle = (style: string): style is GlStyle => (GL_STYLES as string[]).includes(style)

/** One measurement from the node, every number 0 to 255: `b` 24 bands, `f`
 * 96 finer rows, `l` and `r` the wave (128 is silence), `v` each side's
 * loudness. */
export interface SoundFrame {
  b: number[]
  f: number[]
  l: number[]
  r: number[]
  v: number[]
}

/** Bands below this are bass, below `MID_TOP` mid, the rest treble (24
 * bands from 40 Hz to 16 kHz). */
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
/** How much of the way to each new wave point it moves a frame. */
const WAVE_FOLLOW = 0.6
/** How long a VU needle takes to swing (seconds), as real ones do. */
const NEEDLE_SECONDS = 0.3
const LOW = [180, 140, 255]
const HIGH = [255, 255, 255]
const GAP = 0.2

const average = (list: number[]) => (list.length ? list.reduce((a, b) => a + b, 0) / list.length : 0)

/** Follows the sound smoothly (up at once, down gently), with bass, mid,
 * treble, beats and the slow motion the shader styles move by, as DJINN GO
 * for Android's `SoundReader` and `VisualMotion` do. */
export class Smoother {
  /** How strongly it reacts (a look's intensity). */
  gain = 1
  bands: number[] = []
  fine: number[] = []
  left: number[] = []
  right: number[] = []
  /** Each side's VU needle, 0 to 1. */
  needles = [0, 0]
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
  private lastFrame: SoundFrame | null = null
  private bassAverage = 0
  private lastBeat = 0

  step(frame: SoundFrame | null, now: number) {
    const elapsed = this.last ? Math.min(0.2, (now - this.last) / 1000) : 0
    this.last = now
    const fall = FALL_PER_SECOND * elapsed
    const follow = (was: number, level: number) => (level > was ? level : Math.max(level, was - fall))
    const gained = (value: number) => Math.min(1, value * this.gain)
    const levels = (list: number[] | undefined, into: number[]) => {
      const length = list?.length ?? into.length
      for (let i = 0; i < length; i++) into[i] = follow(into[i] ?? 0, gained((list?.[i] ?? 0) / 255))
    }
    levels(frame?.b, this.bands)
    levels(frame?.f, this.fine)
    const wave = (list: number[] | undefined, into: number[]) => {
      const length = list?.length ?? into.length
      for (let i = 0; i < length; i++) {
        const point = list ? Math.max(-1, Math.min(1, (list[i] / 127.5 - 1) * this.gain)) : 0
        into[i] = (into[i] ?? 0) + (point - (into[i] ?? 0)) * WAVE_FOLLOW
      }
    }
    wave(frame?.l, this.left)
    wave(frame?.r, this.right)
    const swing = 1 - Math.exp(-elapsed / NEEDLE_SECONDS)
    this.needles = this.needles.map((was, i) => was + (gained((frame?.v[i] ?? 0) / 255) - was) * swing)

    const raw = (frame?.b ?? []).map((l) => gained(l / 255))
    this.bass = follow(this.bass, average(raw.slice(0, BASS_TOP)))
    this.mid = follow(this.mid, average(raw.slice(BASS_TOP, MID_TOP)))
    this.treble = follow(this.treble, average(raw.slice(MID_TOP)))
    // Beats, from each new measurement.
    this.beat = Math.max(0, this.beat - BEAT_FALL_PER_SECOND * elapsed)
    if (frame && frame !== this.lastFrame) {
      this.lastFrame = frame
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

function mix(a: number[], b: number[], t: number): number[] {
  return a.map((v, i) => Math.round(v + (b[i] - v) * t))
}

function paint(level: number, hueAt: number | null = null): string {
  const t = Math.min(1, Math.max(0, level * level))
  // Rainbow: the color turns slowly across the picture, whiter when loud.
  const low = hueAt === null ? LOW : hslToRgb(hueAt)
  const [r, g, b] = mix(low, HIGH, hueAt === null ? t : t * 0.6)
  return `rgb(${r} ${g} ${b})`
}

function hslToRgb(turn: number): number[] {
  const h = (((turn % 1) + 1) % 1) * 6
  const x = 1 - Math.abs((h % 2) - 1)
  const [r, g, b] = h < 1 ? [1, x, 0] : h < 2 ? [x, 1, 0] : h < 3 ? [0, 1, x] : h < 4 ? [0, x, 1] : h < 5 ? [x, 0, 1] : [1, 0, x]
  return [r, g, b].map((v) => Math.round(70 + v * 185))
}

/** How long rainbow colors take to go all the way around. */
const RAINBOW_MS = 30_000

/** Dark purple through red and orange to pale yellow, as spectrograms are
 * often drawn. */
const HEAT = [
  [0, 0, 4],
  [40, 11, 84],
  [101, 21, 110],
  [159, 42, 99],
  [212, 72, 66],
  [245, 125, 21],
  [250, 193, 39],
  [252, 255, 164],
]

function heat(level: number): number[] {
  const at = Math.min(1, Math.max(0, level)) * (HEAT.length - 1)
  const i = Math.min(HEAT.length - 2, Math.floor(at))
  return mix(HEAT[i], HEAT[i + 1], at - i)
}

/** A rainbow color for `t` (0 to 1). */
function hue(t: number, light = 60, alpha = 1): string {
  return `hsl(${Math.round((((t % 1) + 1) % 1) * 360)} 90% ${light}% / ${alpha})`
}

interface Spark {
  x: number
  y: number
  vx: number
  vy: number
  life: number
  color: string
  size: number
}

/** Draws the shape styles on a 2D canvas, keeping what some need between
 * frames (the spectrogram's picture, the sparks). */
export class ShapeVisual {
  /** Colors turn slowly (a look's "rainbow"). */
  rainbow = false
  private now = 0
  private history: HTMLCanvasElement | null = null
  private column: ImageData | null = null
  private sparks: Spark[] = []
  private lastDraw = 0
  private scrolled = 0

  constructor(
    private ctx: CanvasRenderingContext2D,
    public style: ShapeStyle,
  ) {}

  /** A level's color at `position` (0 to 1) across the picture. */
  private paint(level: number, position: number): string {
    return paint(level, this.rainbow ? this.now / RAINBOW_MS + position * 0.35 : null)
  }

  /** A rainbow color, turned on with the clock when colors rotate. */
  private hue(t: number, light = 60, alpha = 1): string {
    return hue(this.rainbow ? t + this.now / RAINBOW_MS : t, light, alpha)
  }

  draw(sound: Smoother, now: number) {
    this.now = now
    const elapsed = this.lastDraw ? Math.min(0.1, (now - this.lastDraw) / 1000) : 0
    this.lastDraw = now
    const { width, height } = this.ctx.canvas
    // These fade what was there, for trails.
    if (this.style === 'spectrogram') return this.spectrogram(sound, elapsed)
    if (this.style === 'scope') return this.scope(sound)
    if (this.style === 'particles') return this.particles(sound, elapsed)
    this.ctx.clearRect(0, 0, width, height)
    if (this.style === 'wave') return this.wave(sound)
    if (this.style === 'mountain') return this.mountain(sound)
    if (this.style === 'vu') return this.vu(sound)
    if (this.style === 'circle') return this.circle(sound, now)
    if (this.style === 'ring') return this.ring(sound)
    this.bars(sound)
  }

  private bars(sound: Smoother) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    // Music keeps most bands well up the scale; squaring gives them room to move.
    const bands = sound.bands.map((b) => b * b)
    const count = bands.length
    if (!count) return
    const gap = (width / count) * GAP
    const barWidth = width / count - gap
    for (let i = 0; i < count; i++) {
      ctx.fillStyle = this.paint(bands[i], i / count)
      const x = i * (barWidth + gap) + gap / 2
      if (this.style === 'mirror') {
        const h = Math.max(barWidth / 2, (height / 2) * bands[i])
        roundRect(ctx, x, height / 2 - h, barWidth, h * 2, barWidth / 2)
      } else {
        const h = Math.max(barWidth, height * 0.85 * bands[i])
        roundRect(ctx, x, height - h, barWidth, h, barWidth / 2)
      }
    }
  }

  private ring(sound: Smoother) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    const bands = sound.bands.map((b) => b * b)
    const count = bands.length
    if (!count) return
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
      ctx.strokeStyle = this.paint(bands[band], band / count)
      ctx.beginPath()
      ctx.moveTo(cx + Math.cos(angle) * inner, cy + Math.sin(angle) * inner)
      ctx.lineTo(cx + Math.cos(angle) * length, cy + Math.sin(angle) * length)
      ctx.stroke()
    }
  }

  /** The sound wave itself, across the screen (Android's Wave). */
  private wave(sound: Smoother) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    const points = sound.left.map((l, i) => (l + (sound.right[i] ?? 0)) / 2)
    if (points.length < 2) return
    const middle = height / 2
    const reach = height * 0.45
    ctx.beginPath()
    points.forEach((p, i) => {
      const x = (width * i) / (points.length - 1)
      const y = middle - p * reach
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    })
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.lineWidth = Math.max(2, height / 180) * (1 + 1.5 * sound.bass)
    ctx.strokeStyle = this.paint(0.5 + sound.beat / 2, 0)
    ctx.stroke()
  }

  /** Time across, pitch up, loudness as color: the song drawn as it plays,
   * scrolling right to left. */
  private spectrogram(sound: Smoother, elapsed: number) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    const rows = sound.fine.length
    if (!rows || !width || !height) return
    if (!this.history || this.history.width !== width || this.history.height !== height) {
      this.history = document.createElement('canvas')
      this.history.width = width
      this.history.height = height
      const fill = this.history.getContext('2d')
      if (fill) {
        fill.fillStyle = 'rgb(0 0 4)'
        fill.fillRect(0, 0, width, height)
      }
      this.column = null
    }
    const picture = this.history.getContext('2d')
    if (!picture) return
    // About ten seconds across the screen.
    this.scrolled += (width / 10) * elapsed
    const step = Math.floor(this.scrolled)
    if (step > 0) {
      this.scrolled -= step
      picture.drawImage(this.history, -step, 0)
      if (!this.column || this.column.width !== step) this.column = picture.createImageData(step, height)
      const data = this.column.data
      for (let y = 0; y < height; y++) {
        // Low notes at the bottom.
        const row = Math.min(rows - 1, Math.floor(((height - 1 - y) / height) * rows))
        const [r, g, b] = heat(sound.fine[row] ?? 0)
        for (let x = 0; x < step; x++) {
          const at = (y * step + x) * 4
          data[at] = r
          data[at + 1] = g
          data[at + 2] = b
          data[at + 3] = 255
        }
      }
      picture.putImageData(this.column, width - step, 0)
    }
    ctx.drawImage(this.history, 0, 0)
  }

  /** Left against right, turned so the middle of the stereo picture is
   * upright: one line for mono, a cloud for wide stereo. Trails fade. */
  private scope(sound: Smoother) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    ctx.fillStyle = 'rgb(0 0 0 / 0.18)'
    ctx.fillRect(0, 0, width, height)
    const size = Math.min(width, height) * 0.42
    const cx = width / 2
    const cy = height / 2
    ctx.beginPath()
    sound.left.forEach((l, i) => {
      const r = sound.right[i] ?? 0
      const x = cx + ((l - r) / Math.SQRT2) * size * 1.6
      const y = cy - ((l + r) / Math.SQRT2) * size * 1.6
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    })
    const color = this.hue(0.75 + sound.slowMid * 0.3, 65 + sound.beat * 25)
    ctx.lineWidth = Math.max(1.5, height / 300)
    ctx.lineJoin = 'round'
    ctx.strokeStyle = color
    ctx.shadowColor = color
    ctx.shadowBlur = 12
    ctx.stroke()
    ctx.shadowBlur = 0
  }

  /** The finer spectrum as a lit range of hills, with its reflection. */
  private mountain(sound: Smoother) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    const rows = sound.fine.map((v) => v * v)
    if (rows.length < 2) return
    const ground = height * 0.68
    const top = ground * 0.92
    const outline = () => {
      ctx.beginPath()
      ctx.moveTo(0, ground)
      rows.forEach((v, i) => ctx.lineTo((width * i) / (rows.length - 1), ground - v * top))
      ctx.lineTo(width, ground)
      ctx.closePath()
    }
    const fill = ctx.createLinearGradient(0, ground - top, 0, ground)
    fill.addColorStop(0, this.hue(0.8 + sound.slowMid * 0.2, 70))
    fill.addColorStop(0.6, this.hue(0.62 + sound.slowBass * 0.15, 45))
    fill.addColorStop(1, this.hue(0.62, 20, 0.6))
    outline()
    ctx.fillStyle = fill
    ctx.fill()
    ctx.lineWidth = 2
    ctx.strokeStyle = `rgb(255 255 255 / ${0.5 + sound.beat * 0.5})`
    ctx.stroke()
    // The reflection, upside down, squashed and faint.
    ctx.save()
    ctx.translate(0, ground)
    ctx.scale(1, -0.5)
    ctx.translate(0, -ground)
    ctx.globalAlpha = 0.25
    outline()
    ctx.fillStyle = fill
    ctx.fill()
    ctx.restore()
  }

  /** Two needle meters, left and right, swinging as real ones do. */
  private vu(sound: Smoother) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    const meterWidth = Math.min(width * 0.42, height * 1.1)
    const radius = meterWidth * 0.55
    const sweep = Math.PI / 3
    const at = (share: number) => -Math.PI / 2 - sweep + share * 2 * sweep
    sound.needles.forEach((level, side) => {
      const cx = width / 2 + (side === 0 ? -1 : 1) * (meterWidth / 2 + width * 0.02)
      const cy = height / 2 + radius * 0.45
      ctx.fillStyle = 'rgb(250 238 208 / 0.92)'
      roundRect(ctx, cx - meterWidth / 2, cy - radius * 1.15, meterWidth, radius * 1.35, radius * 0.08)
      // The scale, red at the top end.
      ctx.lineWidth = Math.max(2, radius * 0.025)
      for (const [from, to, color] of [
        [0, 0.75, 'rgb(40 30 20)'],
        [0.75, 1, 'rgb(200 30 30)'],
      ] as const) {
        ctx.beginPath()
        ctx.strokeStyle = color
        ctx.arc(cx, cy, radius * 0.85, at(from), at(to))
        ctx.stroke()
      }
      for (let tick = 0; tick <= 10; tick++) {
        const angle = at(tick / 10)
        ctx.beginPath()
        ctx.strokeStyle = tick >= 8 ? 'rgb(200 30 30)' : 'rgb(40 30 20)'
        ctx.moveTo(cx + Math.cos(angle) * radius * 0.85, cy + Math.sin(angle) * radius * 0.85)
        ctx.lineTo(cx + Math.cos(angle) * radius * 0.95, cy + Math.sin(angle) * radius * 0.95)
        ctx.stroke()
      }
      ctx.fillStyle = 'rgb(40 30 20)'
      ctx.font = `${Math.round(radius * 0.12)}px system-ui, sans-serif`
      ctx.textAlign = 'center'
      ctx.fillText(side === 0 ? 'L' : 'R', cx, cy - radius * 0.35)
      // The needle.
      const angle = at(Math.min(1, Math.max(0, level)))
      ctx.beginPath()
      ctx.strokeStyle = 'rgb(20 15 10)'
      ctx.lineWidth = Math.max(2, radius * 0.018)
      ctx.moveTo(cx, cy)
      ctx.lineTo(cx + Math.cos(angle) * radius, cy + Math.sin(angle) * radius)
      ctx.stroke()
      ctx.beginPath()
      ctx.fillStyle = 'rgb(20 15 10)'
      ctx.arc(cx, cy, radius * 0.05, 0, 2 * Math.PI)
      ctx.fill()
    })
  }

  /** The wave wrapped around a ring that swells with the bass. */
  private circle(sound: Smoother, now: number) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    const points = sound.left.map((l, i) => (l + (sound.right[i] ?? 0)) / 2)
    if (points.length < 2) return
    const cx = width / 2
    const cy = height / 2
    const base = Math.min(width, height) * (0.25 + 0.06 * sound.slowBass + 0.03 * sound.beat)
    const reach = Math.min(width, height) * 0.14
    const turn = now / 8000
    // Around once, then back the same way, so the ends meet.
    const loop = [...points, ...[...points].reverse()]
    for (const [scale, alpha] of [
      [1, 1],
      [0.82, 0.45],
    ] as const) {
      ctx.beginPath()
      loop.forEach((p, i) => {
        const angle = (2 * Math.PI * i) / loop.length + turn
        const r = (base + p * reach) * scale
        const x = cx + Math.cos(angle) * r
        const y = cy + Math.sin(angle) * r
        if (i === 0) ctx.moveTo(x, y)
        else ctx.lineTo(x, y)
      })
      ctx.closePath()
      ctx.lineWidth = Math.max(2, height / 220) * (1 + sound.bass)
      ctx.strokeStyle = this.hue(turn * 0.2 + scale * 0.3, 65, alpha)
      ctx.stroke()
    }
  }

  /** Sparks thrown out from the middle, a burst on every beat, colored by
   * pitch. */
  private particles(sound: Smoother, elapsed: number) {
    const ctx = this.ctx
    const { width, height } = ctx.canvas
    ctx.fillStyle = 'rgb(0 0 0 / 0.22)'
    ctx.fillRect(0, 0, width, height)
    const scale = Math.min(width, height)
    const count = Math.round(sound.bass * 4 + (sound.beat > 0.95 ? 60 : 0))
    const bands = Math.max(1, sound.bands.length)
    for (let i = 0; i < count && this.sparks.length < 1500; i++) {
      const band = Math.floor(Math.random() * bands)
      const angle = Math.random() * 2 * Math.PI
      const speed = scale * (0.15 + Math.random() * 0.5) * (0.5 + (sound.bands[band] ?? 0))
      this.sparks.push({
        x: width / 2,
        y: height / 2,
        vx: Math.cos(angle) * speed,
        vy: Math.sin(angle) * speed,
        life: 1,
        color: this.hue((band / bands) * 0.8 + 0.55, 65),
        size: scale * (0.003 + Math.random() * 0.006) * (1 + sound.treble),
      })
    }
    this.sparks = this.sparks.filter((spark) => {
      spark.x += spark.vx * elapsed
      spark.y += spark.vy * elapsed
      spark.vx *= 1 - 0.6 * elapsed
      spark.vy *= 1 - 0.6 * elapsed
      spark.life -= elapsed * 0.6
      if (spark.life <= 0) return false
      ctx.globalAlpha = spark.life
      ctx.fillStyle = spark.color
      ctx.beginPath()
      ctx.arc(spark.x, spark.y, spark.size, 0, 2 * Math.PI)
      ctx.fill()
      return true
    })
    ctx.globalAlpha = 1
  }
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath()
  ctx.roundRect(x, y, w, h, Math.min(r, h / 2))
  ctx.fill()
}
