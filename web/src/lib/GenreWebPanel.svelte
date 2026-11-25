<script lang="ts">
  // Admin > Genres > Family tree: DJINN's own genre web drawn for checking
  // by eye. Families open into genres, genres into styles, and so on; picking
  // one shows where it sits, what it grew out of, what it's close to, what
  // it was a reaction against, and how many of the library's songs land on
  // it. The web itself is written in crates/djinn-library/genre_web.
  import { onMount, tick } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { WebGenre, WebTree } from './types'

  let tree = $state<WebTree | null>(null)
  let open = $state<Set<number>>(new Set())
  let picked = $state<number | null>(null)
  let filter = $state('')
  let mineOnly = $state(false)
  let showUnplaced = $state(false)
  let list = $state<HTMLElement>()

  const n = (x: number) => x.toLocaleString(locale.code)

  onMount(async () => {
    try {
      tree = await api.genreWeb()
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
  /** Songs on each genre and everything below it, each counted once. */
  const below = $derived(genres.map((g) => g.songs_below))
  const families = $derived(genres.flatMap((g, i) => (g.parent === null ? [i] : [])))

  /** Links pointing at each genre from others, for the "other way" lists. */
  const reverse = $derived.by(() => {
    const r = genres.map(() => ({ from: [] as number[], near: [] as number[], against: [] as number[] }))
    genres.forEach((g, i) => {
      for (const x of g.from) r[x].from.push(i)
      for (const x of g.near) r[x].near.push(i)
      for (const x of g.against) r[x].against.push(i)
    })
    return r
  })

  const plain = (s: string) =>
    s
      .normalize('NFD')
      .replace(/\p{M}/gu, '')
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]+/gu, ' ')
      .trim()

  /** With a filter: the genres matching it, and every genre above them. */
  const shown = $derived.by(() => {
    const words = plain(filter)
    if (!words && !mineOnly) return null
    const keep = new Set<number>()
    genres.forEach((g, i) => {
      const hit = !words || [g.name, ...g.aliases].some((name) => plain(name).includes(words))
      if (!hit || (mineOnly && below[i] === 0)) return
      for (let at: number | null = i; at !== null && !keep.has(at); at = genres[at].parent) keep.add(at)
    })
    return keep
  })

  function isOpen(i: number): boolean {
    return open.has(i) || (shown !== null && plain(filter) !== '')
  }

  function toggle(i: number) {
    const next = new Set(open)
    if (next.has(i)) next.delete(i)
    else next.add(i)
    open = next
  }

  function openAll(on: boolean) {
    open = on ? new Set(genres.flatMap((_, i) => (children[i].length ? [i] : []))) : new Set()
  }

  /** Picks a genre, opening the branches above it and scrolling to it. */
  async function jump(i: number) {
    const next = new Set(open)
    for (let at = genres[i].parent; at !== null; at = genres[at].parent) next.add(at)
    open = next
    picked = i
    await tick()
    list?.querySelector<HTMLElement>(`[data-genre="${i}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }

  let detail = $state<HTMLElement>()

  /** Picks a genre from the tree; on a phone the details are above it. */
  async function pick(i: number) {
    picked = i
    await tick()
    if (window.matchMedia('(max-width: 900px)').matches) detail?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

  function years(g: WebGenre): string {
    if (g.from_year === null) return ''
    return g.until_year === null ? t('gw.since', { year: g.from_year }) : t('gw.years', { from: g.from_year, until: g.until_year })
  }

  function path(i: number): number[] {
    const up: number[] = []
    for (let at = genres[i].parent; at !== null; at = genres[at].parent) up.unshift(at)
    return up
  }

  const merge = (a: number[], b: number[]) => [...new Set([...a, ...b])]
</script>

{#snippet branch(i: number)}
  {@const g = genres[i]}
  {#if shown === null || shown.has(i)}
    <li>
      <div class="row" class:picked={picked === i} data-genre={i}>
        {#if children[i].length}
          <button class="toggle" aria-expanded={isOpen(i)} aria-label={isOpen(i) ? t('gw.close') : t('gw.open')} onclick={() => toggle(i)}>
            {isOpen(i) ? '▾' : '▸'}
          </button>
        {:else}
          <span class="toggle"></span>
        {/if}
        <button class="name" class:family={g.parent === null} class:empty={below[i] === 0} onclick={() => pick(i)}>
          {g.name}
        </button>
        {#if g.from.length + g.near.length + g.against.length}
          <span class="links" title={t('gw.links_title')}>
            {#if g.from.length}<span class="from">&lt;{g.from.length}</span>{/if}
            {#if g.near.length}<span class="near">~{g.near.length}</span>{/if}
            {#if g.against.length}<span class="against">!{g.against.length}</span>{/if}
          </span>
        {/if}
        {#if below[i]}<span class="songs">{n(below[i])}</span>{/if}
      </div>
      {#if children[i].length && isOpen(i)}
        <ul>
          {#each children[i] as c (c)}{@render branch(c)}{/each}
        </ul>
      {/if}
    </li>
  {/if}
{/snippet}

{#snippet chips(ids: number[])}
  <div class="chips">
    {#each ids as id (id)}
      <button class="chip" onclick={() => jump(id)}>{genres[id].name}</button>
    {/each}
  </div>
{/snippet}

<section class="card wide">
  <h2>{t('gw.title')}</h2>
  <p class="small muted">{t('gw.intro')}</p>
  {#if !tree}
    <p class="muted">{t('common.loading')}</p>
  {:else}
    <div class="tools">
      <input type="search" placeholder={t('gw.find')} bind:value={filter} />
      <label class="switch"><input type="checkbox" bind:checked={mineOnly} /> {t('gw.mine_only')}</label>
      <button onclick={() => openAll(true)}>{t('gw.open_all')}</button>
      <button onclick={() => openAll(false)}>{t('gw.close_all')}</button>
    </div>
    <p class="small muted">
      {t('gw.counts', { genres: n(genres.length), families: n(families.length) })}
      <span class="legend"><span class="from">&lt;</span> {t('gw.grew_from')} <span class="near">~</span> {t('gw.close_to')} <span class="against">!</span> {t('gw.against')}</span>
    </p>
    <div class="layout">
      <ul class="tree" bind:this={list}>
        {#each families as f (f)}{@render branch(f)}{/each}
      </ul>
      <aside class="detail" bind:this={detail}>
        {#if picked === null}
          <p class="muted small">{t('gw.pick')}</p>
        {:else}
          {@const g = genres[picked]}
          {@const back = reverse[picked]}
          {#if g.parent !== null}
            <div class="crumbs small">
              {#each path(picked) as up (up)}
                <button class="link" onclick={() => jump(up)}>{genres[up].name}</button> ›
              {/each}
            </div>
          {/if}
          <h3>{g.name}</h3>
          {#if years(g)}<p class="small">{years(g)}</p>{/if}
          <p class="small">{t('gw.songs_here', { songs: n(g.songs) })}{#if below[picked] !== g.songs} · {t('gw.songs_below', { songs: n(below[picked]) })}{/if}</p>
          {#if g.aliases.length}
            <h4>{t('gw.aliases')}</h4>
            <p class="small">{g.aliases.join(', ')}</p>
          {/if}
          {#if children[picked].length}
            <h4>{t('gw.kinds')}</h4>
            {@render chips(children[picked])}
          {/if}
          {#if g.from.length}
            <h4><span class="from">&lt;</span> {t('gw.grew_from')}</h4>
            {@render chips(g.from)}
          {/if}
          {#if back.from.length}
            <h4>{t('gw.grew_into')}</h4>
            {@render chips(back.from)}
          {/if}
          {#if g.near.length || back.near.length}
            <h4><span class="near">~</span> {t('gw.close_to')}</h4>
            {@render chips(merge(g.near, back.near))}
          {/if}
          {#if g.against.length}
            <h4><span class="against">!</span> {t('gw.against')}</h4>
            {@render chips(g.against)}
          {/if}
          {#if back.against.length}
            <h4>{t('gw.reacted_to_by')}</h4>
            {@render chips(back.against)}
          {/if}
        {/if}
      </aside>
    </div>
    {#if tree.unplaced.length}
      <h3>{t('gw.unplaced', { count: n(tree.unplaced.length) })}</h3>
      <p class="small muted">{t('gw.unplaced_note')}</p>
      <div class="unplaced">
        {#each showUnplaced ? tree.unplaced : tree.unplaced.slice(0, 40) as [name, songs] (name)}
          <span class="tag">{name} <span class="songs">{n(songs)}</span></span>
        {/each}
      </div>
      {#if tree.unplaced.length > 40}
        <button class="link" onclick={() => (showUnplaced = !showUnplaced)}>
          {showUnplaced ? t('gw.show_fewer') : t('gw.show_all')}
        </button>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .wide {
    grid-column: 1 / -1;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    margin: 0.5rem 0;
  }
  .tools input[type='search'] {
    flex: 1 1 14rem;
  }
  .legend {
    margin-left: 0.75rem;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 22rem);
    gap: 1rem;
    align-items: start;
  }
  @media (min-width: 901px) {
    .detail {
      position: sticky;
      top: 1rem;
    }
  }
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: 1fr;
    }
    .detail {
      order: -1;
    }
  }
  .tree,
  .tree ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tree {
    max-height: 70vh;
    overflow: auto;
  }
  .tree ul {
    padding-left: 1.1rem;
    border-left: 1px solid var(--edge);
    margin-left: 0.6rem;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.1rem 0.25rem;
    border-radius: 6px;
  }
  .row.picked {
    background: var(--card-bg);
    box-shadow: 0 0 0 1px var(--edge-strong);
  }
  .row button,
  .chip,
  .link {
    min-height: 0;
  }
  .toggle {
    width: 1.3rem;
    flex: none;
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    cursor: pointer;
  }
  .name {
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }
  .name.family {
    font-weight: 700;
  }
  .name.empty {
    opacity: 0.55;
  }
  .links {
    font-size: 0.75rem;
    display: flex;
    gap: 0.3rem;
  }
  .songs {
    margin-left: auto;
    font-size: 0.8rem;
    opacity: 0.7;
  }
  .from {
    color: var(--accent-2);
  }
  .near {
    color: var(--accent);
  }
  .against {
    color: #ff5c7a;
  }
  .detail {
    padding: 0.75rem;
    border: 1px solid var(--edge);
    border-radius: 10px;
  }
  .detail h3 {
    margin: 0.25rem 0;
  }
  .detail h4 {
    margin: 0.75rem 0 0.3rem;
    font-size: 0.85rem;
  }
  .crumbs {
    opacity: 0.8;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  .chip {
    font-size: 0.8rem;
    padding: 0.15rem 0.55rem;
    border-radius: 999px;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent-2);
    cursor: pointer;
  }
  .unplaced {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0.5rem 0;
  }
  .tag {
    font-size: 0.8rem;
    padding: 0.15rem 0.55rem;
    border: 1px solid var(--edge);
    border-radius: 999px;
  }
  .tag .songs {
    margin-left: 0.25rem;
  }
</style>
