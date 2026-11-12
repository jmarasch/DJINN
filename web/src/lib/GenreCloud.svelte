<script lang="ts">
  // The genre cloud: each genre a dot sized by its number of songs, with
  // lines between related genres (songs in common, similar sound). It opens on
  // the biggest genres. Tap one to center on it, with what's near it and what's
  // near those; tap the center to open its page, or another dot to move on.
  // Each view is laid out by a small force simulation from a fixed spiral, so
  // the same library always looks the same. The top level opens at full size so
  // labels stay readable on a phone; drag to move around, and pinch, scroll or
  // use the buttons to zoom, without zooming the page.
  import { untrack } from 'svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import type { GenreMap } from './types'

  const TOP_CHOICES = [50, 100, 200, 300]
  const LINE_CHOICES = [
    { label: 'Fewer', links: 2 },
    { label: 'Normal', links: 4 },
    { label: 'More', links: 8 },
  ]
  /** A zoomed view's genres near the center, and near each of those. */
  const NEAR = 14
  const NEAR_THOSE = 5
  const MOST_IN_VIEW = 50
  const MOVE_MS = 350

  function stored(key: string, choices: number[], fallback: number) {
    try {
      const value = Number(localStorage.getItem(key))
      return choices.includes(value) ? value : fallback
    } catch {
      return fallback
    }
  }

  function store(key: string, value: number) {
    try {
      localStorage.setItem(key, String(value))
    } catch {
      // Only a convenience.
    }
  }

  let top = $state(stored('djinn-genre-top', TOP_CHOICES, 100))
  let links = $state(
    stored(
      'djinn-genre-lines',
      LINE_CHOICES.map((c) => c.links),
      4,
    ),
  )
  let map = $state<GenreMap | null>(null)
  let failed = $state(false)
  /** How you got here, from the top level; the last is at the center. Each step is a place in the browser's history. */
  let path = $state<string[]>(history.state?.genreCloud ?? [])

  $effect(() => {
    const [t, l] = [top, links]
    store('djinn-genre-top', t)
    store('djinn-genre-lines', l)
    api
      .genres(t, l)
      .then((m) => {
        map = m
        failed = false
      })
      .catch(() => (failed = true))
  })

  $effect(() => {
    const back = (e: PopStateEvent) => (path = e.state?.genreCloud ?? [])
    window.addEventListener('popstate', back)
    return () => window.removeEventListener('popstate', back)
  })

  function go(next: string[]) {
    path = next
    history.pushState({ ...history.state, genreCloud: next }, '')
  }

  /** Centers on a genre; one already on the way goes back to it. */
  function centerOn(name: string) {
    const at = path.indexOf(name)
    go(at >= 0 ? path.slice(0, at + 1) : [...path, name])
  }

  function pressed(e: MouseEvent, name: string) {
    // The center's link opens its page.
    if (name === center) return
    e.preventDefault()
    centerOn(name)
  }

  const empty: GenreMap = { genres: [], links: [], top_links: [] }

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
  const center = $derived(path.length ? path[path.length - 1] : null)
  const layout = $derived(lay(map ?? empty, center, top))
  /** Where each dot was last view, and where a new one comes from. */
  let last = new Map<string, { x: number; y: number }>()
  let from = $state(new Map<string, { x: number; y: number }>())
  let origin = $state({ x: 0, y: 0 })
  let moved = $state(1)
  const drawn = $derived(
    layout.dots.map((d) => {
      const start = from.get(d.name) ?? origin
      return { x: start.x + (d.x - start.x) * moved, y: start.y + (d.y - start.y) * moved }
    }),
  )
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

  // Start in the middle of the view: at full size at the top level, and with
  // all of it showing when zoomed. Dots move from where they were.
  $effect(() => {
    const l = layout
    const zoomed = path.length > 0
    let frame = 0
    untrack(() => {
      cx = l.left + l.width / 2
      cy = l.top + l.height / 2
      zoom = zoomed ? fit : 1
      hovered = null
      const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches
      from = last
      origin = (center && last.get(center)) || { x: 0, y: 0 }
      last = new Map(l.dots.map((d) => [d.name, { x: d.x, y: d.y }]))
      if (still || !from.size) {
        moved = 1
        return
      }
      moved = 0
      const began = performance.now()
      const step = (now: number) => {
        const done = Math.min(1, (now - began) / MOVE_MS)
        moved = 1 - (1 - done) ** 3
        if (done < 1) frame = requestAnimationFrame(step)
      }
      frame = requestAnimationFrame(step)
    })
    return () => cancelAnimationFrame(frame)
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

  /** A genre and what's near it, and near those, strongest first; the genre comes first. */
  function around(map: GenreMap, at: number) {
    const near = (g: number) =>
      map.links
        .filter((l) => l.a === g || l.b === g)
        .sort((x, y) => y.strength - x.strength)
        .map((l) => (l.a === g ? l.b : l.a))
    const first = near(at).slice(0, NEAR)
    const shown = new Set([at, ...first])
    for (const g of first) {
      for (const next of near(g).slice(0, NEAR_THOSE)) {
        if (shown.size >= MOST_IN_VIEW) break
        shown.add(next)
      }
    }
    return [...shown]
  }

  function lay(map: GenreMap, center: string | null, biggest: number) {
    const at = center === null ? -1 : map.genres.findIndex((g) => g.name === center)
    const shown =
      at >= 0
        ? around(map, at)
        : // The biggest genres, ties by name as on the server.
          map.genres
            .map((_, i) => i)
            .sort((a, b) => map.genres[b].songs - map.genres[a].songs)
            .slice(0, biggest)
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
    const lines: Line[] = (at >= 0 ? map.links : map.top_links)
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

{#if failed}
  <p class="small muted">Could not load the genres.</p>
{/if}
<nav class="trail small" aria-label="Where you are in the cloud">
  {#if path.length}
    <button class="link" onclick={() => go([])}>Top</button>
    {#each path as name, i (name)}
      <span class="muted">/</span>
      {#if i === path.length - 1}
        <strong>{name}</strong>
      {:else}
        <button class="link" onclick={() => go(path.slice(0, i + 1))}>{name}</button>
      {/if}
    {/each}
  {:else if map}
    <span class="muted">Top {layout.dots.length} of {map.genres.length} genres. Bigger means more songs; lines join related genres.</span>
  {/if}
</nav>
<div class="choices small">
  <label>
    Genres at the top
    <select bind:value={top}>
      {#each TOP_CHOICES as choice (choice)}<option value={choice}>{choice}</option>{/each}
    </select>
  </label>
  <label>
    Lines
    <select bind:value={links}>
      {#each LINE_CHOICES as choice (choice.links)}<option value={choice.links}>{choice.label}</option>{/each}
    </select>
  </label>
</div>

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
      {@const a = drawn[line.a]}
      {@const b = drawn[line.b]}
      {#if a && b}
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
      {/if}
    {/each}
    {#each layout.dots as dot, i (dot.name)}
      {@const place = drawn[i] ?? dot}
      <a
        href="#/genre/{encodeURIComponent(dot.name)}"
        class:dim={near !== null && !near.has(i)}
        class:center={dot.name === center}
        onclick={(e) => pressed(e, dot.name)}
        onpointerenter={() => (hovered = i)}
        onpointerleave={() => (hovered = null)}
        onfocus={() => (hovered = i)}
        onblur={() => (hovered = null)}
      >
        <title>{dot.name}: {dot.songs} {dot.songs === 1 ? 'song' : 'songs'}</title>
        <circle cx={place.x} cy={place.y} r={dot.r} />
        <text x={place.x} y={place.y + dot.r + dot.font} font-size={dot.font}>{dot.name}</text>
      </a>
    {/each}
  </svg>
  <button class="corner top-left" aria-label="Back to the top" disabled={!path.length} onclick={() => go([])}>
    <Icon name="home" size={20} />
  </button>
  <button class="corner top-right" aria-label="Fit this view" onclick={showAll}><Icon name="fit" size={20} /></button>
  <button class="corner bottom-left" aria-label="Zoom in" onclick={() => zoomBy(1.4)}><Icon name="plus" size={20} /></button>
  <button class="corner bottom-right" aria-label="Zoom out" onclick={() => zoomBy(1 / 1.4)}><Icon name="minus" size={20} /></button>
</div>
<p class="hint small muted">
  {center
    ? `Showing ${center} and what's near it. Tap ${center} to open it, or another genre to move there.`
    : "Tap a genre to see what's near it."}
  Drag to move around; pinch, scroll or use + and - to zoom.
</p>

<style>
  .cloud {
    position: relative;
    height: min(70vh, 640px);
    overflow: hidden;
    border: 1px solid var(--edge);
    border-radius: 14px;
    background: var(--surface);
    box-shadow: var(--glow);
  }

  .corner {
    position: absolute;
    width: 40px;
    min-height: 40px;
    padding: 0;
    border-radius: 10px;
    background: var(--surface-3);
    color: var(--text);
  }

  .corner:disabled {
    opacity: 0.45;
  }

  .top-left {
    top: 10px;
    left: 10px;
  }

  .top-right {
    top: 10px;
    right: 10px;
  }

  .bottom-left {
    bottom: 10px;
    left: 10px;
  }

  .bottom-right {
    bottom: 10px;
    right: 10px;
  }

  .trail {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 0 0 4px;
  }

  .link {
    padding: 0;
    min-height: 0;
    border: 0;
    background: transparent;
    color: var(--accent);
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    margin: 0 0 8px;
  }

  .choices label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  a.center circle {
    stroke: var(--text);
    stroke-width: 2;
    fill-opacity: 1;
  }

  .hint {
    margin: 8px 0 0;
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
