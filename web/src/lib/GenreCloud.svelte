<script lang="ts">
  // The genre cloud: each genre a dot sized by its number of songs, with
  // lines between related genres (songs in common, similar sound). Tap one to
  // open its page. Laid out once by a small force simulation from a fixed
  // spiral, so the same library always looks the same. Drawn at full size in
  // a scrolling box, so labels stay readable on a phone.
  import type { GenreMap } from './types'

  let { map, limit = 80 }: { map: GenreMap; limit?: number } = $props()

  interface Dot {
    name: string
    songs: number
    x: number
    y: number
    vx: number
    vy: number
    r: number
    font: number
  }
  interface Line {
    a: number
    b: number
    strength: number
  }

  const STEPS = 320
  const layout = $derived(lay(map, limit))
  let hovered = $state<number | null>(null)
  let box = $state<HTMLDivElement | null>(null)
  /** The drawing is bigger than the box, so it scrolls. */
  let overflows = $state(false)

  /** Dots linked to the hovered one, which stay bright. */
  const near = $derived.by(() => {
    if (hovered === null) return null
    const set = new Set([hovered])
    for (const l of layout.lines) {
      if (l.a === hovered) set.add(l.b)
      if (l.b === hovered) set.add(l.a)
    }
    return set
  })

  // Start scrolled to the middle of the cloud.
  $effect(() => {
    if (!box || !layout.dots.length) return
    box.scrollLeft = (box.scrollWidth - box.clientWidth) / 2
    box.scrollTop = (box.scrollHeight - box.clientHeight) / 2
    overflows = box.scrollWidth > box.clientWidth + 1 || box.scrollHeight > box.clientHeight + 1
  })

  function lay(map: GenreMap, limit: number) {
    // The biggest genres, if there are a lot.
    const shown = map.genres
      .map((_, i) => i)
      .sort((a, b) => map.genres[b].songs - map.genres[a].songs)
      .slice(0, limit)
    const index = new Map(shown.map((g, i) => [g, i]))
    const most = Math.max(1, ...shown.map((g) => map.genres[g].songs))
    const dots: Dot[] = shown.map((g, i) => {
      const share = Math.sqrt(map.genres[g].songs / most)
      // Golden-angle spiral, biggest in the middle.
      const angle = i * 2.39996
      const out = 34 * Math.sqrt(i)
      return {
        name: map.genres[g].name,
        songs: map.genres[g].songs,
        x: Math.cos(angle) * out,
        y: Math.sin(angle) * out,
        vx: 0,
        vy: 0,
        r: 5 + 15 * share,
        font: 12 + 7 * share,
      }
    })
    const lines: Line[] = map.links
      .filter((l) => index.has(l.a) && index.has(l.b))
      .map((l) => ({ a: index.get(l.a)!, b: index.get(l.b)!, strength: l.strength }))
    simulate(dots, lines)
    separate(dots)

    // Fit the drawing to the dots and their labels.
    const pad = 24
    let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity]
    for (const d of dots) {
      const half = Math.max(d.r, labelWidth(d) / 2)
      left = Math.min(left, d.x - half)
      right = Math.max(right, d.x + half)
      top = Math.min(top, d.y - d.r)
      bottom = Math.max(bottom, d.y + d.r + d.font + 6)
    }
    if (!dots.length) [left, top, right, bottom] = [0, 0, 0, 0]
    const width = right - left + pad * 2
    const height = bottom - top + pad * 2
    return { dots, lines, width, height, viewBox: `${left - pad} ${top - pad} ${width} ${height}` }
  }

  const labelWidth = (d: Dot) => d.name.length * d.font * 0.56

  /** The space a dot and its label take: [left, top, right, bottom]. */
  function bounds(d: Dot): [number, number, number, number] {
    const half = Math.max(d.r, labelWidth(d) / 2) + 4
    return [d.x - half, d.y - d.r - 2, d.x + half, d.y + d.r + d.font + 6]
  }

  /** Nudges apart dots whose labels still overlap after the layout, each
   * time along the shorter way out. */
  function separate(dots: Dot[]) {
    for (let round = 0; round < 200; round++) {
      let moved = false
      for (let i = 0; i < dots.length; i++) {
        for (let j = i + 1; j < dots.length; j++) {
          const [a, b] = [bounds(dots[i]), bounds(dots[j])]
          const wide = Math.min(a[2], b[2]) - Math.max(a[0], b[0])
          const tall = Math.min(a[3], b[3]) - Math.max(a[1], b[1])
          if (wide <= 0 || tall <= 0) continue
          moved = true
          if (wide < tall) {
            const step = (wide / 2 + 0.5) * (dots[i].x < dots[j].x ? -1 : 1)
            dots[i].x += step
            dots[j].x -= step
          } else {
            const step = (tall / 2 + 0.5) * (dots[i].y < dots[j].y ? -1 : 1)
            dots[i].y += step
            dots[j].y -= step
          }
        }
      }
      if (!moved) break
    }
  }

  function simulate(dots: Dot[], lines: Line[]) {
    for (let step = 0; step < STEPS; step++) {
      const heat = 1 - step / STEPS
      for (let i = 0; i < dots.length; i++) {
        for (let j = i + 1; j < dots.length; j++) {
          const [a, b] = [dots[i], dots[j]]
          let dx = b.x - a.x
          let dy = b.y - a.y
          const dist = Math.max(Math.hypot(dx, dy), 0.01)
          dx /= dist
          dy /= dist
          // Everyone pushes apart, harder when labels would overlap.
          let push = 3200 / (dist * dist)
          const room = a.r + b.r + Math.max(labelWidth(a), labelWidth(b)) * 0.6
          if (dist < room) push += (room - dist) * 0.2
          a.vx -= dx * push
          a.vy -= dy * push
          b.vx += dx * push
          b.vy += dy * push
        }
      }
      // Related genres pull together, the more related the closer.
      for (const l of lines) {
        const [a, b] = [dots[l.a], dots[l.b]]
        const dx = b.x - a.x
        const dy = b.y - a.y
        const dist = Math.max(Math.hypot(dx, dy), 0.01)
        const rest = 170 - 70 * l.strength + a.r + b.r
        const pull = (dist - rest) * 0.012 * (0.4 + l.strength)
        a.vx += (dx / dist) * pull
        a.vy += (dy / dist) * pull
        b.vx -= (dx / dist) * pull
        b.vy -= (dy / dist) * pull
      }
      for (const d of dots) {
        // A gentle pull to the middle keeps loners nearby; sideways a bit
        // harder, so the cloud comes out taller than wide, like a phone.
        d.vx -= d.x * 0.01
        d.vy -= d.y * 0.004
        d.vx *= 0.82
        d.vy *= 0.82
        const speed = Math.hypot(d.vx, d.vy)
        const cap = 30 * heat + 1
        const scale = speed > cap ? cap / speed : 1
        d.x += d.vx * scale
        d.y += d.vy * scale
      }
    }
  }
</script>

{#if overflows}<p class="hint small muted">Drag or scroll to see the rest.</p>{/if}
<div class="cloud" bind:this={box}>
  <svg
    width={layout.width}
    height={layout.height}
    viewBox={layout.viewBox}
    role="img"
    aria-label="Genres and how they relate"
  >
    {#each layout.lines as line, i (i)}
      {@const a = layout.dots[line.a]}
      {@const b = layout.dots[line.b]}
      <line
        x1={a.x}
        y1={a.y}
        x2={b.x}
        y2={b.y}
        stroke-width={1 + 2.5 * line.strength}
        opacity={near && !(near.has(line.a) && near.has(line.b) && (line.a === hovered || line.b === hovered))
          ? 0.05
          : 0.15 + 0.5 * line.strength}
      />
    {/each}
    {#each layout.dots as dot, i (dot.name)}
      <a
        href="#/genre/{encodeURIComponent(dot.name)}"
        class:dim={near !== null && !near.has(i)}
        onpointerenter={() => (hovered = i)}
        onpointerleave={() => (hovered = null)}
        onfocus={() => (hovered = i)}
        onblur={() => (hovered = null)}
      >
        <title>{dot.name}: {dot.songs} {dot.songs === 1 ? 'song' : 'songs'}</title>
        <circle cx={dot.x} cy={dot.y} r={dot.r} />
        <text x={dot.x} y={dot.y + dot.r + dot.font} font-size={dot.font}>{dot.name}</text>
      </a>
    {/each}
  </svg>
</div>

<style>
  .cloud {
    height: min(70vh, 640px);
    overflow: auto;
    border-radius: 14px;
    background: var(--surface);
    -webkit-overflow-scrolling: touch;
  }

  .hint {
    margin: 0 0 6px;
  }

  svg {
    display: block;
    margin: auto;
  }

  line {
    stroke: var(--accent);
    transition: opacity 0.15s;
  }

  circle {
    fill: var(--accent);
    fill-opacity: 0.85;
  }

  text {
    fill: var(--text);
    text-anchor: middle;
  }

  a {
    transition: opacity 0.15s;
  }

  a:hover circle,
  a:focus-visible circle {
    fill-opacity: 1;
  }

  a.dim {
    opacity: 0.25;
  }
</style>
