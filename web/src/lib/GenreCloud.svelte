<script lang="ts">
  // The genre map for listeners: DJINN's genre web as a mind map. It opens
  // on the families (Rock, Metal, Jazz...), joined by lines that sum up how
  // their genres link. Tap one to put it in the middle, with what it's a kind
  // of above it, its kinds below, and the genres it grew out of, is close to
  // or was a reaction against around it, in the same line styles as Admin >
  // Genres > Map; tap the middle one to open its page, or another to move
  // there. The trail always leads back up to the top. Each view is laid out
  // by a small force simulation from fixed starting places, so it looks the
  // same every time. Drag to move around, and pinch, scroll or use the
  // buttons to zoom, without zooming the page.
  import { t } from './i18n.svelte'
  import { untrack } from 'svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import type { WebTree } from './types'

  type Kind = 'tree' | 'from' | 'near' | 'against'
  const MOVE_MS = 350
  /** At the top, how many of each family's strongest ties to other families are drawn... */
  const FAMILY_TIES = 1
  /** And the strongest ties overall. */
  const TOP_TIES = 12
  /** A genre view opens no smaller than this, so names stay readable on a phone. */
  const READABLE = 0.75

  let web = $state<WebTree | null>(null)
  let failed = $state(false)
  /** Genres none of the library's songs land on are left out unless asked for. */
  let everyGenre = $state(storedFlag())
  /** How you got here, from the top level; the last is at the center. Each step is a place in the browser's history. */
  let path = $state<string[]>(history.state?.genreCloud ?? [])

  function storedFlag(): boolean {
    try {
      return localStorage.getItem('djinn-genre-every') === '1'
    } catch {
      return false
    }
  }

  $effect(() => {
    try {
      localStorage.setItem('djinn-genre-every', everyGenre ? '1' : '0')
    } catch {
      // Only a convenience.
    }
  })

  $effect(() => {
    api
      .genreWeb()
      .then((w) => {
        web = w
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

  interface Dot {
    id: number
    name: string
    songs: number
    x: number
    y: number
    hx: number
    hy: number
    vx: number
    vy: number
    r: number
    font: number
    /** In the same family as the center (or a family, at the top). */
    home: boolean
  }
  interface Line {
    a: number
    b: number
    kind: Kind
    /** 0 to 1: how many links a top-level line stands for. */
    strength: number
  }

  const STEPS = 320
  /** Zooming in goes up to this many times full size. */
  const MAX_ZOOM = 3
  /** A press that moves further than this (pixels) is a drag, not a tap. */
  const DRAG_PX = 6
  const center = $derived(path.length ? path[path.length - 1] : null)
  const layout = $derived(lay(web, center, everyGenre))
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
      zoom = zoomed ? Math.max(fit, READABLE) : 1
      // A genre view that doesn't fit opens on the genre in the middle.
      const middle = zoomed && fit < READABLE ? l.dots[0] : null
      cx = middle ? middle.x : l.left + l.width / 2
      cy = middle ? middle.y : l.top + l.height / 2
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

  /** Puts dots where they start and where they're pulled back to. */
  function dot(web: WebTree, id: number, x: number, y: number, most: number, home: boolean, big = false): Dot {
    const g = web.genres[id]
    const share = Math.sqrt(g.songs_below / most)
    return {
      id,
      name: g.name,
      songs: g.songs_below,
      x,
      y,
      hx: x,
      hy: y,
      vx: 0,
      vy: 0,
      r: big ? 22 : 5 + 14 * Math.min(1, share),
      font: big ? 17 : 12 + 5 * Math.min(1, share),
      home,
    }
  }

  /** Places around an arc, from `from` to `to` (radians, 0 is right, y down). */
  function arc(count: number, radius: number, from: number, to: number): { x: number; y: number }[] {
    return Array.from({ length: count }, (_, i) => {
      const a = count === 1 ? (from + to) / 2 : from + ((to - from) * i) / (count - 1)
      // Long rows step out and in, so neighbors' labels don't meet.
      const r = radius + (count > 9 && i % 2 ? 70 : 0)
      return { x: Math.cos(a) * r, y: Math.sin(a) * r }
    })
  }

  function lay(web: WebTree | null, center: string | null, every: boolean) {
    const dots: Dot[] = []
    const lines: Line[] = []
    if (web) {
      const genres = web.genres
      const shows = (i: number) => every || genres[i].songs_below > 0
      const familyOf = (i: number) => {
        let at = i
        while (genres[at].parent !== null) at = genres[at].parent!
        return at
      }
      const at = center === null ? -1 : genres.findIndex((g) => g.name === center)
      if (at < 0) {
        // The families, biggest in the middle, and lines for how their genres link.
        const families = genres
          .flatMap((g, i) => (g.parent === null && shows(i) ? [i] : []))
          .sort((a, b) => genres[b].songs_below - genres[a].songs_below)
        const most = Math.max(1, ...families.map((f) => genres[f].songs_below))
        families.forEach((f, i) => {
          const angle = i * 2.39996
          const out = 60 * Math.sqrt(i)
          dots.push(dot(web, f, Math.cos(angle) * out, Math.sin(angle) * out, most, true))
        })
        const place = new Map(families.map((f, i) => [f, i]))
        const pairs = new Map<string, { a: number; b: number; kinds: Record<string, number>; total: number }>()
        genres.forEach((g, i) => {
          const add = (other: number, kind: Kind) => {
            const [fa, fb] = [place.get(familyOf(i)), place.get(familyOf(other))]
            if (fa === undefined || fb === undefined || fa === fb) return
            // "Grew out of" runs from the older family to the newer one.
            const [a, b] = kind === 'from' ? [fb, fa] : [fa, fb]
            const key = `${Math.min(a, b)}-${Math.max(a, b)}`
            const pair = pairs.get(key) ?? { a, b, kinds: {}, total: 0 }
            pair.kinds[kind] = (pair.kinds[kind] ?? 0) + 1
            pair.total++
            pairs.set(key, pair)
          }
          for (const x of g.from) add(x, 'from')
          for (const x of g.near) add(x, 'near')
          for (const x of g.against) add(x, 'against')
        })
        // Each family's few strongest ties, so the overview stays readable.
        const all = [...pairs.values()].sort((x, y) => y.total - x.total)
        const strongest = Math.max(1, all[0]?.total ?? 1)
        const kept = new Set<(typeof all)[number]>(all.slice(0, TOP_TIES))
        families.forEach((_, f) => {
          all.filter((p) => p.a === f || p.b === f).slice(0, FAMILY_TIES).forEach((p) => kept.add(p))
        })
        for (const pair of kept) {
          const kind = (Object.entries(pair.kinds).sort((x, y) => y[1] - x[1])[0]?.[0] ?? 'near') as Kind
          lines.push({ a: pair.a, b: pair.b, kind, strength: Math.sqrt(pair.total / strongest) })
        }
      } else {
        // The center, what it's a kind of above, its kinds below, and its
        // links around it.
        const family = familyOf(at)
        const most = Math.max(1, ...genres.map((g) => (g.parent === at ? g.songs_below : 0)))
        dots.push(dot(web, at, 0, 0, most, true, true))
        const parent = genres[at].parent
        if (parent !== null) dots.push(dot(web, parent, 0, -200, most, true))
        const kids = genres.flatMap((g, i) => (g.parent === at && shows(i) ? [i] : []))
        arc(kids.length, 210, Math.PI * 0.12, Math.PI * 0.88).forEach((p, i) =>
          dots.push(dot(web, kids[i], p.x, p.y, most, true)),
        )
        const shown = new Set(dots.map((d) => d.id))
        const linked: number[] = []
        const g = genres[at]
        const touching = genres.flatMap((h, i) =>
          h.from.includes(at) || h.near.includes(at) || h.against.includes(at) ? [i] : [],
        )
        for (const x of [...g.from, ...g.near, ...g.against, ...touching]) {
          if (!shown.has(x) && !linked.includes(x) && shows(x)) linked.push(x)
        }
        // Around the top half, either side of the parent.
        const left = linked.filter((_, i) => i % 2 === 0)
        const right = linked.filter((_, i) => i % 2 === 1)
        arc(left.length, 270, Math.PI * 0.95, Math.PI * 1.38).forEach((p, i) =>
          dots.push(dot(web, left[i], p.x, p.y, most, familyOf(left[i]) === family)),
        )
        arc(right.length, 270, Math.PI * 1.62, Math.PI * 2.05).forEach((p, i) =>
          dots.push(dot(web, right[i], p.x, p.y, most, familyOf(right[i]) === family)),
        )
        // Only the lines to and from the genre in the middle (dot 0).
        const index = new Map(dots.map((d, i) => [d.id, i]))
        const add = (a: number, b: number, kind: Kind) => {
          if (a === 0 || b === 0) lines.push({ a, b, kind, strength: 1 })
        }
        for (const [i, d] of dots.entries()) {
          const h = genres[d.id]
          if (h.parent !== null && index.has(h.parent)) add(index.get(h.parent)!, i, 'tree')
          for (const x of h.from) if (index.has(x)) add(index.get(x)!, i, 'from')
          for (const x of h.near) if (index.has(x)) add(i, index.get(x)!, 'near')
          for (const x of h.against) if (index.has(x)) add(i, index.get(x)!, 'against')
        }
      }
      simulate(dots, lines, at >= 0)
      separate(dots, at >= 0)
    }

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
  function separate(dots: Dot[], pinFirst: boolean) {
    for (let round = 0; round < 200; round++) {
      let moved = false
      for (let i = 0; i < dots.length; i++) {
        for (let j = i + 1; j < dots.length; j++) {
          const [a, b] = [bounds(dots[i]), bounds(dots[j])]
          const wide = Math.min(a[2], b[2]) - Math.max(a[0], b[0])
          const tall = Math.min(a[3], b[3]) - Math.max(a[1], b[1])
          if (wide <= 0 || tall <= 0) continue
          moved = true
          // The genre in the middle stays put; the other one moves the whole way.
          const [mine, theirs] = pinFirst && i === 0 ? [0, 2] : [1, 1]
          if (wide < tall) {
            const step = (wide / 2 + 0.5) * (dots[i].x < dots[j].x ? -1 : 1)
            dots[i].x += step * mine
            dots[j].x -= step * theirs
          } else {
            const step = (tall / 2 + 0.5) * (dots[i].y < dots[j].y ? -1 : 1)
            dots[i].y += step * mine
            dots[j].y -= step * theirs
          }
        }
      }
      if (!moved) break
    }
  }

  function simulate(dots: Dot[], lines: Line[], centered: boolean) {
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
      // Linked genres pull together a little; each dot is drawn back toward
      // its starting place, so the shape (parent above, kinds below) holds.
      for (const l of lines) {
        const [a, b] = [dots[l.a], dots[l.b]]
        const dx = b.x - a.x
        const dy = b.y - a.y
        const dist = Math.max(Math.hypot(dx, dy), 0.01)
        const rest = 190 + a.r + b.r
        const pull = (dist - rest) * 0.004 * (0.4 + l.strength)
        a.vx += (dx / dist) * pull
        a.vy += (dy / dist) * pull
        b.vx -= (dx / dist) * pull
        b.vy -= (dy / dist) * pull
      }
      for (const [i, d] of dots.entries()) {
        if (centered && i === 0) {
          // The genre in the middle stays there.
          d.vx = 0
          d.vy = 0
          continue
        }
        d.vx += (d.hx - d.x) * (centered ? 0.08 : 0.03)
        d.vy += (d.hy - d.y) * (centered ? 0.08 : 0.03)
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

  /** The center's place in the tree, from its family down to it. */
  const lineage = $derived.by(() => {
    if (!web || center === null) return []
    const genres = web.genres
    let at: number | null = genres.findIndex((g) => g.name === center)
    const up: string[] = []
    while (at !== null && at >= 0) {
      up.unshift(genres[at].name)
      at = genres[at].parent
    }
    return up
  })

  /** A line from dot a to dot b, ending at b's edge so its arrow shows. */
  function ends(a: { x: number; y: number }, b: { x: number; y: number }, r: number) {
    const dx = b.x - a.x
    const dy = b.y - a.y
    const dist = Math.max(Math.hypot(dx, dy), 0.01)
    const cut = Math.min(r + 3, dist / 2)
    return { x1: a.x, y1: a.y, x2: b.x - (dx / dist) * cut, y2: b.y - (dy / dist) * cut }
  }

  const color: Record<Kind, string> = {
    tree: 'var(--text)',
    from: 'var(--accent-2)',
    near: 'var(--accent)',
    against: '#ff5c7a',
  }
  const dash: Record<Kind, string | undefined> = { tree: undefined, from: undefined, near: '2 4', against: '7 4' }
</script>

{#if failed}
  <p class="small muted">{t('gcl.failed')}</p>
{/if}
<nav class="trail small" aria-label={t('gcl.where')}>
  {#if center !== null}
    <button class="link" onclick={() => go([])}>{t('gcl.top')}</button>
    {#each lineage as name, i (name)}
      <span class="muted">›</span>
      {#if i === lineage.length - 1}
        <strong>{name}</strong>
      {:else}
        <button class="link" onclick={() => centerOn(name)}>{name}</button>
      {/if}
    {/each}
  {:else if web}
    <span class="muted">{t('gcl.families', { n: layout.dots.length })}</span>
  {/if}
</nav>
<div class="choices small">
  <label class="switch"><input type="checkbox" bind:checked={everyGenre} /> {t('gcl.every')}</label>
</div>

<div class="cloud" bind:clientWidth={boxWidth} bind:clientHeight={boxHeight}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <svg
    bind:this={svg}
    {viewBox}
    role="img"
    aria-label={t('gcl.svg')}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onclickcapture={click}
  >
    <defs>
      {#each ['from', 'against'] as const as kind (kind)}
        <marker id="gc-arrow-{kind}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M 0 0 L 10 5 L 0 10 z" fill={color[kind]} />
        </marker>
      {/each}
    </defs>
    {#each layout.lines as line, i (i)}
      {@const a = drawn[line.a]}
      {@const b = drawn[line.b]}
      {#if a && b}
        {@const e = ends(a, b, layout.dots[line.b].r)}
        <line
          class="kind-{line.kind}"
          {...e}
          stroke={color[line.kind]}
          stroke-dasharray={dash[line.kind]}
          stroke-width={line.kind === 'tree' ? 1.5 : 1.4 + 2.4 * line.strength}
          marker-end={line.kind === 'from' || line.kind === 'against' ? `url(#gc-arrow-${line.kind})` : undefined}
          opacity={near && !(near.has(line.a) && near.has(line.b) && (line.a === hovered || line.b === hovered))
            ? 0.06
            : line.kind === 'tree'
              ? 0.35
              : 0.4 + 0.5 * line.strength}
        />
      {/if}
    {/each}
    {#each layout.dots as dot, i (dot.name)}
      {@const place = drawn[i] ?? dot}
      <a
        href="#/genre/{encodeURIComponent(dot.name)}"
        class:dim={near !== null && !near.has(i)}
        class:center={dot.name === center}
        class:away={!dot.home}
        class:empty={dot.songs === 0}
        onclick={(e) => pressed(e, dot.name)}
        onpointerenter={() => (hovered = i)}
        onpointerleave={() => (hovered = null)}
        onfocus={() => (hovered = i)}
        onblur={() => (hovered = null)}
      >
        <title>{dot.name}: {t('count.songs', { n: dot.songs })}</title>
        <circle cx={place.x} cy={place.y} r={dot.r} />
        <text x={place.x} y={place.y + dot.r + dot.font} font-size={dot.font}>{dot.name}</text>
      </a>
    {/each}
  </svg>
  <button class="corner top-left" aria-label={t('gcl.back_top')} disabled={!path.length} onclick={() => go([])}>
    <Icon name="home" size={20} />
  </button>
  <button class="corner top-right" aria-label={t('gcl.fit')} onclick={showAll}><Icon name="fit" size={20} /></button>
  <button class="corner bottom-left" aria-label={t('gcl.zoom_in')} onclick={() => zoomBy(1.4)}><Icon name="plus" size={20} /></button>
  <button class="corner bottom-right" aria-label={t('gcl.zoom_out')} onclick={() => zoomBy(1 / 1.4)}><Icon name="minus" size={20} /></button>
</div>
<p class="legend small">
  <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={color.tree} stroke-opacity="0.5" stroke-width="1.5" /></svg>{t('gcl.key_kind')}</span>
  <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={color.from} stroke-width="2" /></svg>{t('gw.grew_from')}</span>
  <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={color.near} stroke-width="2" stroke-dasharray="2 3" /></svg>{t('gw.close_to')}</span>
  <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={color.against} stroke-width="2" stroke-dasharray="6 3" /></svg>{t('gw.against')}</span>
</p>
<p class="hint small muted">
  {center ? t('gcl.showing', { name: center }) : t('gcl.tap')}
  {t('gcl.drag')}
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

  a:focus {
    outline: none;
  }

  a:focus-visible circle {
    stroke: var(--accent-2);
    stroke-width: 3;
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
    transition: opacity 0.15s;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem 1rem;
    align-items: center;
    margin: 8px 0 0;
  }

  .key {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .key svg {
    width: 26px;
    height: 10px;
    touch-action: auto;
    cursor: default;
  }

  a.away circle {
    fill: var(--accent-2);
  }

  a.empty circle {
    fill-opacity: 0.3;
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
