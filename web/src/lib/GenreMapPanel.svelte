<script lang="ts">
  // Admin > Genres > Map: one family of DJINN's genre web drawn as a picture.
  // Each genre is a row, indented under the genre it's a kind of, with tree
  // lines; arcs on the right join genres that grew out of one another (with
  // an arrow toward the newer one), are close, or were a reaction against one
  // another. Links into other families are tags beside the genre, which open
  // that family. Closed branches carry their genres' links, so a family's
  // overview shows how its main genres relate; picking a genre lights up its
  // links and fades the rest.
  import { onMount, tick } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { WebTree } from './types'

  type Kind = 'from' | 'near' | 'against'
  /** An arc between two rows: `a` to `b`, the arrow (if any) at `b`. */
  type Arc = { a: number; b: number; kind: Kind; count: number }
  /** A link into another family, shown beside the row's genre. */
  type Tag = { at: number; other: number; kind: Kind }
  /** What a row shows in the tag column: one tag per family it links into,
   * or, for the picked row, one per genre. */
  type Shown = { label: string; kind: Kind; go: number | null; x: number; w: number }

  const ROW = 28
  const INDENT = 22
  const TOP = 16
  const LEFT = 14
  const FONT = 13

  let tree = $state<WebTree | null>(null)
  let family = $state(0)
  let closed = $state<Set<number>>(new Set())
  let picked = $state<number | null>(null)
  let mineOnly = $state(false)
  let scale = $state(1)
  let box = $state<HTMLElement>()

  const n = (x: number) => x.toLocaleString(locale.code)

  onMount(async () => {
    try {
      tree = await api.genreWeb()
      const first = tree.genres.findIndex((g) => g.parent === null)
      showFamily(Math.max(first, 0))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gw.load_failed'), true)
    }
  })

  const genres = $derived(tree?.genres ?? [])
  const children = $derived.by(() => {
    const kids: number[][] = genres.map(() => [])
    genres.forEach((g, i) => {
      if (g.parent !== null) kids[g.parent].push(i)
    })
    return kids
  })
  const familyOf = $derived.by(() => {
    const top: number[] = []
    genres.forEach((g, i) => {
      top[i] = g.parent === null ? i : top[g.parent]
    })
    return top
  })
  const families = $derived(genres.flatMap((g, i) => (g.parent === null ? [i] : [])))

  /** Opens a family with only its genres showing (their styles closed). */
  function showFamily(f: number) {
    family = f
    picked = null
    const shut = new Set<number>()
    genres.forEach((g, i) => {
      if (familyOf[i] === f && g.depth >= 1 && children[i].length) shut.add(i)
    })
    closed = shut
    if (box) box.scrollTo({ top: 0, left: 0 })
  }

  /** The rows shown: the family's genres in tree order, skipping closed branches. */
  const rows = $derived.by(() => {
    const out: number[] = []
    if (!genres.length) return out
    const walk = (i: number) => {
      if (mineOnly && genres[i].songs_below === 0 && i !== family) return
      out.push(i)
      if (!closed.has(i)) for (const c of children[i]) walk(c)
    }
    walk(family)
    return out
  })
  const rowOf = $derived(new Map(rows.map((id, r) => [id, r])))

  /** Where a genre shows: itself, or the closed branch it's folded into. */
  function shownAs(i: number): number | null {
    for (let at: number | null = i; at !== null; at = genres[at].parent) {
      if (rowOf.has(at)) return at
    }
    return null
  }

  const rank: Record<Kind, number> = { near: 0, against: 1, from: 2 }

  const links = $derived.by(() => {
    const arcs = new Map<string, Arc>()
    const tags = new Map<string, Tag>()
    const add = (from: number, to: number, kind: Kind) => {
      // Arrows run from the older genre to the newer one ("grew out of"),
      // and from the one reacting to what it reacted against.
      const [x, y] = kind === 'from' ? [to, from] : [from, to]
      const inX = familyOf[x] === family
      const inY = familyOf[y] === family
      if (inX && inY) {
        const a = shownAs(x)
        const b = shownAs(y)
        if (a === null || b === null || a === b) return
        const key = kind === 'near' ? `${Math.min(a, b)}-${Math.max(a, b)}-near` : `${a}-${b}-${kind}`
        const arc = arcs.get(key)
        if (arc) arc.count++
        else arcs.set(key, { a, b, kind, count: 1 })
      } else if (inX || inY) {
        const mine = inX ? x : y
        const other = inX ? y : x
        const at = shownAs(mine)
        if (at === null) return
        const key = `${at}-${other}`
        const had = tags.get(key)
        // One tag per genre, showing its strongest link.
        if (!had || rank[kind] > rank[had.kind]) tags.set(key, { at, other, kind })
      }
    }
    genres.forEach((g, i) => {
      for (const x of g.from) add(i, x, 'from')
      for (const x of g.near) add(i, x, 'near')
      for (const x of g.against) add(i, x, 'against')
    })
    return { arcs: [...arcs.values()], tags: [...tags.values()] }
  })

  // Text widths, measured once per name.
  let measure: CanvasRenderingContext2D | null = null
  const widths = new Map<string, number>()
  function textWidth(text: string, size = FONT): number {
    const key = `${size}|${text}`
    const known = widths.get(key)
    if (known !== undefined) return known
    if (!measure) {
      measure = document.createElement('canvas').getContext('2d')
    }
    let w = text.length * size * 0.55
    if (measure) {
      measure.font = `${size}px ${getComputedStyle(document.body).fontFamily}`
      w = measure.measureText(text).width
    }
    widths.set(key, w)
    return w
  }

  /** Each row's place and where its name ends. */
  const layout = $derived.by(() => {
    const place = new Map<number, { x: number; y: number; end: number }>()
    rows.forEach((id, r) => {
      const g = genres[id]
      const x = LEFT + g.depth * INDENT
      const y = TOP + r * ROW + ROW / 2
      let end = x + 10 + textWidth(g.name, g.depth === 0 ? FONT + 2 : FONT)
      if (g.songs_below) end += 8 + textWidth(n(g.songs_below), 11)
      place.set(id, { x, y, end: end + 6 })
    })
    return place
  })

  const bulge = (dy: number) => 28 + Math.min(Math.abs(dy) * 0.3, 320)

  /** Where the tag column starts: past every name and arc. */
  const tagX = $derived.by(() => {
    let x = 200
    for (const p of layout.values()) x = Math.max(x, p.end)
    for (const arc of links.arcs) {
      const a = layout.get(arc.a)
      const b = layout.get(arc.b)
      if (a && b) x = Math.max(x, Math.max(a.end, b.end) + bulge(b.y - a.y))
    }
    return x + 16
  })

  /** The tags each row shows. */
  const shownTags = $derived.by(() => {
    const byRow = new Map<number, Tag[]>()
    for (const tag of links.tags) {
      const list = byRow.get(tag.at) ?? []
      list.push(tag)
      byRow.set(tag.at, list)
    }
    const out = new Map<number, Shown[]>()
    for (const [row, list] of byRow) {
      const shown: Omit<Shown, 'x' | 'w'>[] = []
      if (row === picked || list.length === 1) {
        for (const tag of list) {
          shown.push({ label: `${genres[tag.other].name} · ${genres[familyOf[tag.other]].name}`, kind: tag.kind, go: tag.other })
        }
      } else {
        const perFamily = new Map<number, Tag[]>()
        for (const tag of list) {
          const f = familyOf[tag.other]
          perFamily.set(f, [...(perFamily.get(f) ?? []), tag])
        }
        for (const [f, tags] of perFamily) {
          const kind = tags.reduce<Kind>((k, tag) => (rank[tag.kind] > rank[k] ? tag.kind : k), 'near')
          shown.push(
            tags.length === 1
              ? { label: `${genres[tags[0].other].name} · ${genres[f].name}`, kind, go: tags[0].other }
              : { label: `${genres[f].name} ${tags.length}`, kind, go: null },
          )
        }
      }
      let x = tagX
      out.set(
        row,
        shown.map((t) => {
          const w = textWidth(t.label, 11) + 14
          const placed = { ...t, x, w }
          x += w + 6
          return placed
        }),
      )
    }
    return out
  })

  const size = $derived.by(() => {
    let w = tagX
    for (const tags of shownTags.values()) {
      const last = tags[tags.length - 1]
      if (last) w = Math.max(w, last.x + last.w)
    }
    return { w: w + 24, h: TOP * 2 + rows.length * ROW }
  })

  function arcPath(arc: Arc): string {
    const a = layout.get(arc.a)
    const b = layout.get(arc.b)
    if (!a || !b) return ''
    const cx = Math.max(a.end, b.end) + bulge(b.y - a.y)
    return `M ${a.end} ${a.y} C ${cx} ${a.y}, ${cx} ${b.y}, ${b.end + 4} ${b.y}`
  }

  /** What the picked genre touches, to light up. */
  const lit = $derived.by(() => {
    if (picked === null) return null
    const on = new Set<number>([picked])
    for (const arc of links.arcs) {
      if (arc.a === picked) on.add(arc.b)
      if (arc.b === picked) on.add(arc.a)
    }
    return on
  })
  const arcLit = (arc: Arc) => picked === null || arc.a === picked || arc.b === picked

  function toggle(i: number) {
    const next = new Set(closed)
    if (next.has(i)) next.delete(i)
    else next.add(i)
    closed = next
  }

  function openAll(on: boolean) {
    if (on) closed = new Set()
    else showFamily(family)
  }

  /** Shows a genre: its family, the branches above it open, scrolled to it. */
  async function goTo(i: number) {
    if (familyOf[i] !== family) showFamily(familyOf[i])
    const next = new Set(closed)
    for (let at = genres[i].parent; at !== null; at = genres[at].parent) next.delete(at)
    closed = next
    picked = i
    await tick()
    const p = layout.get(i)
    if (box && p) box.scrollTo({ top: Math.max(0, p.y * scale - box.clientHeight / 2), behavior: 'smooth' })
  }

  // Dragging the background moves the picture.
  let drag = $state<{ x: number; y: number; left: number; top: number } | null>(null)
  function down(e: PointerEvent) {
    if (!box || e.button !== 0 || (e.target as Element).closest('[data-hit]')) return
    drag = { x: e.clientX, y: e.clientY, left: box.scrollLeft, top: box.scrollTop }
    box.setPointerCapture(e.pointerId)
  }
  function move(e: PointerEvent) {
    if (!drag || !box) return
    box.scrollLeft = drag.left - (e.clientX - drag.x)
    box.scrollTop = drag.top - (e.clientY - drag.y)
  }
  function up() {
    drag = null
  }

  const zoom = (by: number) => (scale = Math.min(2, Math.max(0.4, Math.round((scale + by) * 10) / 10)))

  const kindColor: Record<Kind, string> = {
    from: 'var(--accent-2)',
    near: 'var(--accent)',
    against: '#ff5c7a',
  }
</script>

<section class="card wide">
  <h2>{t('gm.title')}</h2>
  <p class="small muted">{t('gm.intro')}</p>
  {#if !tree}
    <p class="muted">{t('common.loading')}</p>
  {:else}
    <div class="families">
      {#each families as f (f)}
        <button class:active={f === family} onclick={() => showFamily(f)}>
          {genres[f].name}
          {#if genres[f].songs_below}<span class="count">{n(genres[f].songs_below)}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="tools">
      <label class="switch"><input type="checkbox" bind:checked={mineOnly} /> {t('gw.mine_only')}</label>
      <button onclick={() => openAll(true)}>{t('gw.open_all')}</button>
      <button onclick={() => openAll(false)}>{t('gw.close_all')}</button>
      <span class="zoom">
        <button aria-label={t('gm.smaller')} onclick={() => zoom(-0.2)}>−</button>
        <span class="small">{Math.round(scale * 100)}%</span>
        <button aria-label={t('gm.bigger')} onclick={() => zoom(0.2)}>+</button>
      </span>
    </div>
    <p class="small legend">
      <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={kindColor.from} stroke-width="2" /></svg>{t('gm.key_from')}</span>
      <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={kindColor.near} stroke-width="2" stroke-dasharray="2 3" /></svg>{t('gw.close_to')}</span>
      <span class="key"><svg width="26" height="10"><line x1="1" y1="5" x2="25" y2="5" stroke={kindColor.against} stroke-width="2" stroke-dasharray="6 3" /></svg>{t('gm.key_against')}</span>
      <span class="muted">{t('gm.how')}</span>
    </p>
    {#if picked !== null}
      {@const g = genres[picked]}
      <p class="picked small">
        <strong>{g.name}</strong>
        {#if g.from_year !== null}
          · {g.until_year === null ? t('gw.since', { year: g.from_year }) : t('gw.years', { from: g.from_year, until: g.until_year })}
        {/if}
        · {t('gm.songs', { songs: n(g.songs_below) })}
        {#if g.aliases.length}· {t('gw.aliases')}: {g.aliases.join(', ')}{/if}
        <button class="link" onclick={() => (picked = null)}>{t('gm.clear')}</button>
      </p>
    {/if}
    <div
      class="map"
      class:dragging={drag !== null}
      bind:this={box}
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointercancel={up}
      role="presentation"
    >
      <svg width={size.w * scale} height={size.h * scale} viewBox="0 0 {size.w} {size.h}">
        <defs>
          {#each ['from', 'against'] as const as kind (kind)}
            <marker id="gm-arrow-{kind}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 10 5 L 0 10 z" fill={kindColor[kind]} />
            </marker>
          {/each}
        </defs>
        <!-- Tree lines. -->
        {#each rows as id (id)}
          {@const g = genres[id]}
          {#if g.parent !== null && layout.get(g.parent) && layout.get(id)}
            {@const p = layout.get(g.parent)!}
            {@const c = layout.get(id)!}
            <path class="branch" class:faded={lit !== null && !lit.has(id)} d="M {p.x} {p.y + 5} V {c.y} H {c.x - 5}" />
          {/if}
        {/each}
        <!-- Links across the tree. -->
        {#each links.arcs as arc (`${arc.a}-${arc.b}-${arc.kind}`)}
          <path
            class="arc"
            class:faded={!arcLit(arc)}
            d={arcPath(arc)}
            stroke={kindColor[arc.kind]}
            stroke-width={Math.min(1.4 + arc.count * 0.5, 4)}
            stroke-dasharray={arc.kind === 'near' ? '2 3' : arc.kind === 'against' ? '6 3' : undefined}
            marker-end={arc.kind === 'near' ? undefined : `url(#gm-arrow-${arc.kind})`}
          />
        {/each}
        <!-- Genres. -->
        {#each rows as id (id)}
          {@const g = genres[id]}
          {@const p = layout.get(id)}
          {#if p}
            <g class:faded={lit !== null && !lit.has(id)}>
              {#if children[id].length}
                <g class="toggle" data-hit role="button" tabindex="0" aria-label={closed.has(id) ? t('gw.open') : t('gw.close')}
                  onclick={() => toggle(id)} onkeydown={(e) => e.key === 'Enter' && toggle(id)}>
                  <circle cx={p.x} cy={p.y} r="6" class:shut={closed.has(id)} />
                  <path d={closed.has(id) ? `M ${p.x - 3} ${p.y} H ${p.x + 3} M ${p.x} ${p.y - 3} V ${p.y + 3}` : `M ${p.x - 3} ${p.y} H ${p.x + 3}`} />
                </g>
              {:else}
                <circle cx={p.x} cy={p.y} r="3" class="leaf" />
              {/if}
              <text
                data-hit
                role="button"
                tabindex="0"
                x={p.x + 10}
                y={p.y + 4}
                class="name"
                class:family={g.depth === 0}
                class:empty={g.songs_below === 0}
                class:picked={picked === id}
                onclick={() => (picked = picked === id ? null : id)}
                onkeydown={(e) => e.key === 'Enter' && (picked = id)}
              >{g.name}{#if g.songs_below}<tspan class="count" dx="8">{n(g.songs_below)}</tspan>{/if}</text>
              {#if shownTags.get(id)?.length}
                <line class="guide" x1={p.end} y1={p.y} x2={tagX - 6} y2={p.y} />
              {/if}
              {#each shownTags.get(id) ?? [] as tag (tag.label)}
                <g class="tag" data-hit role="button" tabindex="0"
                  onclick={() => (tag.go === null ? (picked = id) : goTo(tag.go))}
                  onkeydown={(e) => e.key === 'Enter' && (tag.go === null ? (picked = id) : goTo(tag.go))}>
                  <rect x={tag.x} y={p.y - 9} width={tag.w} height="18" rx="9" stroke={kindColor[tag.kind]}
                    stroke-dasharray={tag.kind === 'near' ? '2 2' : tag.kind === 'against' ? '5 2' : undefined} />
                  <text x={tag.x + 7} y={p.y + 4}>{tag.label}</text>
                </g>
              {/each}
            </g>
          {/if}
        {/each}
      </svg>
    </div>
  {/if}
</section>

<style>
  .wide {
    grid-column: 1 / -1;
  }
  .families {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0.5rem 0;
  }
  .families button {
    min-height: 0;
    padding: 0.3rem 0.7rem;
    border-radius: 999px;
    font-size: 0.85rem;
  }
  .families button.active {
    background: var(--accent);
    color: var(--accent-text);
  }
  .families .count {
    opacity: 0.7;
    font-size: 0.75rem;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    margin: 0.5rem 0;
  }
  .zoom {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    margin-left: auto;
  }
  .zoom button {
    min-height: 0;
    width: 2.2rem;
    height: 2.2rem;
    padding: 0;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem 1rem;
    align-items: center;
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .picked {
    margin: 0.25rem 0 0.5rem;
  }
  .link {
    min-height: 0;
    background: none;
    border: none;
    padding: 0 0 0 0.5rem;
    color: var(--accent-2);
    cursor: pointer;
  }
  .map {
    max-height: 75vh;
    overflow: auto;
    border: 1px solid var(--edge);
    border-radius: 10px;
    cursor: grab;
    touch-action: pan-x pan-y;
    user-select: none;
  }
  .map.dragging {
    cursor: grabbing;
  }
  svg {
    display: block;
  }
  .branch {
    fill: none;
    stroke: var(--edge);
    stroke-width: 1.2;
  }
  .arc {
    fill: none;
    opacity: 0.85;
  }
  .faded {
    opacity: 0.12;
  }
  .toggle {
    cursor: pointer;
  }
  .toggle circle {
    fill: var(--card-bg);
    stroke: var(--accent);
    stroke-width: 1.5;
  }
  .toggle circle.shut {
    fill: var(--accent);
  }
  .toggle path {
    stroke: var(--text, currentColor);
    stroke-width: 1.5;
  }
  .toggle circle.shut + path {
    stroke: var(--accent-text);
  }
  .leaf {
    fill: var(--edge-strong);
  }
  .name {
    font-size: 13px;
    fill: currentColor;
    cursor: pointer;
  }
  .name.family {
    font-size: 15px;
    font-weight: 700;
  }
  .name.empty {
    opacity: 0.5;
  }
  .name.picked {
    fill: var(--accent);
    font-weight: 700;
  }
  .count {
    font-size: 11px;
    opacity: 0.6;
    font-weight: 400;
  }
  .tag {
    cursor: pointer;
  }
  .guide {
    stroke: var(--edge);
    stroke-width: 1;
    stroke-dasharray: 1 4;
  }
  .tag rect {
    fill: var(--card-bg);
    stroke-width: 1.3;
  }
  .tag text {
    font-size: 11px;
    fill: currentColor;
  }
</style>
