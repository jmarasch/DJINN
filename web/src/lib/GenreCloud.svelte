<script lang="ts">
  // The genre cloud: each genre a dot sized by its number of songs, with
  // lines between related genres (songs in common, similar sound). Tap one to
  // open its page. Laid out once by a small force simulation from a fixed
  // spiral, so the same library always looks the same. It opens at full size
  // so labels stay readable on a phone; drag to move around, and pinch,
  // scroll or use the buttons to zoom, without zooming the page.
  import Icon from './Icon.svelte'
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
  /** Zooming in goes up to this many times full size. */
  const MAX_ZOOM = 3
  /** A press that moves further than this (pixels) is a drag, not a tap. */
  const DRAG_PX = 6
  const layout = $derived(lay(map, limit))
  let hovered = $state<number | null>(null)
  let svg = $state<SVGSVGElement>()
  let boxWidth = $state(0)
  let boxHeight = $state(0)
  /** The point of the drawing at the middle of the box, and how many screen
   * pixels one unit of the drawing takes. */
  let cx = $state(0)
  let cy = $state(0)
  let zoom = $state(1)
  /** Pointers down on the drawing, and where they were last. */
  const pointers = new Map<number, { x: number; y: number }>()
  let dragged = false
  let pressedAt: { x: number; y: number } | null = null

  /** Small enough to see the whole cloud (but not bigger than full size). */
  const fit = $derived(
    boxWidth && boxHeight ? Math.min(1, boxWidth / layout.width, boxHeight / layout.height) : 1,
  )
  const viewBox = $derived(
    `${cx - boxWidth / 2 / zoom} ${cy - boxHeight / 2 / zoom} ${boxWidth / zoom} ${boxHeight / zoom}`,
  )

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

  // Start at full size in the middle of the cloud.
  $effect(() => {
    const l = layout
    cx = l.left + l.width / 2
    cy = l.top + l.height / 2
    zoom = 1
  })

  /** Zooms by `factor`, keeping the point under (x, y) (pixels in the box)
   * where it is. */
  function zoomBy(factor: number, x = boxWidth / 2, y = boxHeight / 2) {
    const next = Math.max(fit * 0.9, Math.min(MAX_ZOOM, zoom * factor))
    const [px, py] = [cx + (x - boxWidth / 2) / zoom, cy + (y - boxHeight / 2) / zoom]
    zoom = next
    cx = px - (x - boxWidth / 2) / zoom
    cy = py - (y - boxHeight / 2) / zoom
    keepInView()
  }

  function moveBy(dx: number, dy: number) {
    cx -= dx / zoom
    cy -= dy / zoom
    keepInView()
  }

  /** Keeps some of the cloud in the box. */
  function keepInView() {
    const l = layout
    cx = Math.max(l.left, Math.min(l.left + l.width, cx))
    cy = Math.max(l.top, Math.min(l.top + l.height, cy))
  }

  function showAll() {
    cx = layout.left + layout.width / 2
    cy = layout.top + layout.height / 2
    zoom = fit
  }

  function local(e: PointerEvent | WheelEvent) {
    const box = svg?.getBoundingClientRect()
    return { x: e.clientX - (box?.left ?? 0), y: e.clientY - (box?.top ?? 0) }
  }

  function down(e: PointerEvent) {
    if (pointers.size === 0) {
      dragged = false
      pressedAt = local(e)
    }
    pointers.set(e.pointerId, local(e))
  }

  function move(e: PointerEvent) {
    const last = pointers.get(e.pointerId)
    if (!last) return
    const now = local(e)
    if (!dragged && pressedAt && Math.hypot(now.x - pressedAt.x, now.y - pressedAt.y) > DRAG_PX) {
      dragged = true
      // Only now, so a plain tap still reaches the genre's link.
      svg?.setPointerCapture(e.pointerId)
    }
    if (pointers.size === 2) {
      // Pinch: zoom by how much the fingers spread, around their middle,
      // and move with the middle.
      const [a, b] = [...pointers.entries()].map(([id, p]) => (id === e.pointerId ? now : p))
      const [oa, ob] = [...pointers.values()]
      const before = Math.hypot(oa.x - ob.x, oa.y - ob.y)
      const after = Math.hypot(a.x - b.x, a.y - b.y)
      const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
      const oldMid = { x: (oa.x + ob.x) / 2, y: (oa.y + ob.y) / 2 }
      dragged = true
      if (before > 0) zoomBy(after / before, mid.x, mid.y)
      moveBy(mid.x - oldMid.x, mid.y - oldMid.y)
    } else if (dragged) {
      moveBy(now.x - last.x, now.y - last.y)
    }
    pointers.set(e.pointerId, now)
  }

  function up(e: PointerEvent) {
    pointers.delete(e.pointerId)
  }

  function wheel(e: WheelEvent) {
    e.preventDefault()
    const { x, y } = local(e)
    // Trackpads send pinches as wheel events with ctrl held, in small steps.
    const step = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY
    zoomBy(Math.exp(-step * (e.ctrlKey ? 0.01 : 0.002)), x, y)
  }

  // A drag that ends on a genre doesn't open it.
  function click(e: MouseEvent) {
    if (!dragged) return
    e.preventDefault()
    e.stopPropagation()
    dragged = false
  }

  // Wheel and gesture listeners can't be passive, or the page zooms too.
  $effect(() => {
    if (!svg) return
    const el = svg
    const stop = (e: Event) => e.preventDefault()
    el.addEventListener('wheel', wheel, { passive: false })
    el.addEventListener('gesturestart', stop)
    el.addEventListener('gesturechange', stop)
    return () => {
      el.removeEventListener('wheel', wheel)
      el.removeEventListener('gesturestart', stop)
      el.removeEventListener('gesturechange', stop)
    }
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
    return { dots, lines, width, height, left: left - pad, top: top - pad }
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

<p class="hint small muted">Drag to move around; pinch, scroll or use + and - to zoom.</p>
<div class="cloud" bind:clientWidth={boxWidth} bind:clientHeight={boxHeight}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <svg
    bind:this={svg}
    {viewBox}
    role="img"
    aria-label="Genres and how they relate"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onclickcapture={click}
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
  <div class="zoom">
    <button aria-label="Zoom in" onclick={() => zoomBy(1.4)}><Icon name="plus" size={20} /></button>
    <button aria-label="Zoom out" onclick={() => zoomBy(1 / 1.4)}><Icon name="minus" size={20} /></button>
    <button aria-label="Show all" onclick={showAll}><Icon name="fit" size={20} /></button>
  </div>
</div>

<style>
  .cloud {
    position: relative;
    height: min(70vh, 640px);
    overflow: hidden;
    border-radius: 14px;
    background: var(--surface);
  }

  .zoom {
    position: absolute;
    top: 10px;
    right: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .zoom button {
    width: 40px;
    min-height: 40px;
    padding: 0;
    border-radius: 10px;
    background: var(--surface-3);
    color: var(--text);
  }

  .hint {
    margin: 0 0 6px;
  }

  svg {
    display: block;
    width: 100%;
    height: 100%;
    /* The drawing handles pinches and drags itself, so the page doesn't
       zoom or scroll under the finger. */
    touch-action: none;
    cursor: grab;
    user-select: none;
    -webkit-user-select: none;
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
