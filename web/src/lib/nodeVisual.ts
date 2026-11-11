// The visualizer a node draws on its own screen, from the levels its speaker
// plays (`/api/node/levels`). The shapes and colors follow DJINN GO for
// Android's Bars, Ring and Mirror.

export type VisualStyle = 'bars' | 'ring' | 'mirror'

/** How fast a band falls back, in full heights a second. */
const FALL_PER_SECOND = 1.6
const LOW = [180, 140, 255]
const HIGH = [255, 255, 255]
const GAP = 0.2

/** Follows the levels smoothly: up at once, down gently. */
export class Smoother {
  bands: number[] = []
  bass = 0
  private last = 0

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
    const low = this.bands.slice(0, 4)
    this.bass = low.length ? low.reduce((a, b) => a + b, 0) / low.length : 0
  }
}

function paint(level: number): string {
  const t = Math.min(1, Math.max(0, level * level))
  const [r, g, b] = LOW.map((low, i) => Math.round(low + (HIGH[i] - low) * t))
  return `rgb(${r} ${g} ${b})`
}

/** Draws one frame of `style` filling the canvas. */
export function draw(ctx: CanvasRenderingContext2D, style: VisualStyle, sound: Smoother) {
  const { width, height } = ctx.canvas
  ctx.clearRect(0, 0, width, height)
  // Music keeps most bands well up the scale; squaring gives them room to move.
  const bands = sound.bands.map((b) => b * b)
  const count = bands.length
  if (!count) return
  if (style === 'ring') {
    const cx = width / 2
    const cy = height / 2
    const inner = Math.min(width, height) * (0.22 + 0.04 * sound.bass)
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
