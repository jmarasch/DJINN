<script lang="ts" generics="T">
  import { tick, untrack, type Snippet } from 'svelte'
  import { listCache, rowSizes } from './listCache'
  import type { Page } from './types'

  let {
    cacheKey,
    load,
    item,
    key,
    count,
    grid = false,
    empty = 'Nothing here yet.',
    inset = 0,
  }: {
    /** Identifies this list (and its sort) in the cache. */
    cacheKey: string
    load: (offset: number) => Promise<Page<T>>
    item: Snippet<[T]>
    key: (item: T) => number
    /** Caption for the total, e.g. n => `${n} artists`. */
    count?: (total: number) => string
    grid?: boolean
    empty?: string
    /** Height of anything stuck to the top of the page, which a jump
     * scrolls below. */
    inset?: number
  } = $props()

  /** Starts fetching the next page this far before the end scrolls into view. */
  const PRELOAD_PX = 800
  /** The server's page size, used when loading the stretch above a jump. */
  const PAGE = 50
  /** Rows kept on the page beyond each edge of the screen. */
  const OVERSCAN_ROWS = 12
  /** Items shown before a row has been measured. */
  const FIRST_RENDER = 30

  // Only the rows near the screen are on the page, with padding standing in
  // for the rest, so a long list costs the same to open and scroll as a
  // short one. Raw: items are replaced, never changed in place.
  let items = $state.raw<T[]>([])
  /** Where `items` starts in the whole list: after a jump, the songs above
   * load as the page scrolls up. */
  let start = $state(0)
  let total = $state<number | null>(null)
  let loading = $state(false)
  let loadingEarlier = $state(false)
  let error = $state('')
  let sentinel = $state<HTMLElement>()
  let topSentinel = $state<HTMLElement>()
  let container = $state<HTMLElement>()
  let generation = 0
  /** Height of a row including the gap to the next, and items in a row. */
  let rowHeight = $state(0)
  let columns = $state(1)
  /** The rows on the page: from `firstRow`, up to `lastRow`. */
  let firstRow = $state(0)
  let lastRow = $state(0)

  const rows = $derived(Math.ceil(items.length / columns))
  const shown = $derived(
    rowHeight ? items.slice(firstRow * columns, lastRow * columns) : items.slice(0, FIRST_RENDER),
  )
  const paddingTop = $derived(rowHeight ? firstRow * rowHeight : 0)
  const paddingBottom = $derived(rowHeight ? Math.max(0, rows - lastRow) * rowHeight : 0)
  const sizeKey = $derived(grid ? 'grid' : 'list')

  /** Works out the rows near the screen from the scroll position. */
  function place() {
    if (!container || !rowHeight) return
    const top = -container.getBoundingClientRect().top
    const first = Math.floor(top / rowHeight) - OVERSCAN_ROWS
    const last = Math.ceil((top + innerHeight) / rowHeight) + OVERSCAN_ROWS
    firstRow = Math.max(0, Math.min(first, Math.max(0, rows - 1)))
    lastRow = Math.max(firstRow, Math.min(last, rows))
  }

  /** Take the next measurement as it is. Otherwise a row only grows while
   * scrolling: if rows ever differ in height, each new measurement would
   * move the rows on the page and change the next one, round and round
   * without the page ever drawing again. */
  let fresh = true

  /** Measures the rows on the page. They should all be the same height. */
  function measure() {
    const children = container?.children
    if (!children?.length) return
    const top = children[0].getBoundingClientRect().top
    let across = 1
    while (across < children.length && children[across].getBoundingClientRect().top === top) across++
    // With one row showing, the row's own height is the best known.
    const height =
      across < children.length
        ? children[across].getBoundingClientRect().top - top
        : children[0].getBoundingClientRect().height
    if (height <= 0) return
    const take = fresh || across !== columns || height > rowHeight + 0.5
    fresh = false
    if (take && (height !== rowHeight || across !== columns)) {
      rowHeight = height
      columns = across
      rowSizes.set(sizeKey, { height, columns: across })
    }
    place()
  }

  /** After the window changes size, rows may be shorter as well as taller. */
  function remeasure() {
    fresh = true
    measure()
  }

  let framed = false
  function schedule() {
    if (framed) return
    framed = true
    requestAnimationFrame(() => {
      framed = false
      place()
    })
  }

  $effect(() => {
    addEventListener('scroll', schedule, { passive: true })
    addEventListener('resize', remeasure, { passive: true })
    return () => {
      removeEventListener('scroll', schedule)
      removeEventListener('resize', remeasure)
    }
  })

  // A size seen before gives the page its full height straight away, so
  // scrolling back to where it was lands in the right place.
  $effect(() => {
    const known = rowSizes.get(sizeKey)
    untrack(() => {
      rowHeight = known?.height ?? 0
      columns = known?.columns ?? 1
      firstRow = 0
      lastRow = known ? Math.ceil((innerHeight * 2) / known.height) : 0
    })
  })

  // After anything that changes which rows there are or how big they are.
  /** Whether the rows last measured were tiles. */
  let shownAs: boolean | undefined
  $effect(() => {
    void shown.length
    const asGrid = grid
    untrack(() => {
      // Tiles and rows are different sizes.
      if (shownAs !== undefined && asGrid !== shownAs) fresh = true
      shownAs = asGrid
      tick().then(measure)
    })
  })

  $effect(() => {
    const k = cacheKey
    untrack(() => {
      generation++
      loading = false
      loadingEarlier = false
      error = ''
      const cached = listCache.get(k)
      if (cached) {
        items = cached.items as T[]
        total = cached.total
        start = cached.start
      } else {
        items = []
        total = null
        start = 0
        more()
      }
    })
  })

  function remember() {
    listCache.set(cacheKey, { items, total: total ?? items.length, start })
  }

  async function more() {
    if (loading || (total !== null && start + items.length >= total)) return
    loading = true
    const gen = generation
    try {
      const page = await load(start + items.length)
      if (gen !== generation) return
      items = [...items, ...page.items]
      // An empty page means the library shrank under us; stop here.
      total = page.items.length ? page.total : start + items.length
      remember()
    } catch (e) {
      if (gen === generation) error = e instanceof Error ? e.message : 'Could not load'
      return
    } finally {
      if (gen === generation) loading = false
    }
    // A short page may leave the end still visible; keep going if so.
    requestAnimationFrame(() => {
      if (sentinel && sentinel.getBoundingClientRect().top < innerHeight + PRELOAD_PX) more()
    })
  }

  /** Loads the page above the loaded stretch, keeping what's on screen in
   * place. */
  async function earlier() {
    if (loadingEarlier || start === 0) return
    loadingEarlier = true
    const gen = generation
    const from = Math.max(0, start - PAGE)
    try {
      const page = await load(from)
      if (gen !== generation) return
      const added = start - from
      items = [...page.items.slice(0, added), ...items]
      start = from
      remember()
      // Keeps what's on screen in place (anchoring is off for the list).
      const moved = (added / columns) * rowHeight
      await tick()
      scrollBy(0, moved)
      place()
    } catch {
      // Scrolling up again retries.
      return
    } finally {
      if (gen === generation) loadingEarlier = false
    }
    requestAnimationFrame(() => {
      if (topSentinel && topSentinel.getBoundingClientRect().bottom > -PRELOAD_PX) earlier()
    })
  }

  /** Scrolls to the entry at `offset` in the whole list, loading the list
   * from there if it isn't loaded yet. */
  export async function jumpTo(offset: number) {
    if (total !== null) offset = Math.max(0, Math.min(offset, total - 1))
    if (offset >= start && offset < start + items.length) {
      reveal(offset - start)
      return
    }
    const gen = ++generation
    loadingEarlier = false
    error = ''
    loading = true
    try {
      const page = await load(offset)
      if (gen !== generation) return
      items = page.items
      start = offset
      total = page.total
      remember()
    } catch (e) {
      if (gen === generation) error = e instanceof Error ? e.message : 'Could not load'
      return
    } finally {
      if (gen === generation) loading = false
    }
    await tick()
    if (gen === generation) reveal(0)
  }

  function reveal(index: number) {
    if (!container) return
    const row = Math.floor(index / columns)
    scrollTo({ top: container.getBoundingClientRect().top + scrollY + row * rowHeight - inset })
    place()
  }

  $effect(() => {
    if (!sentinel) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) more()
      },
      { rootMargin: `${PRELOAD_PX}px` },
    )
    observer.observe(sentinel)
    return () => observer.disconnect()
  })

  $effect(() => {
    if (!topSentinel) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) earlier()
      },
      { rootMargin: `${PRELOAD_PX}px` },
    )
    observer.observe(topSentinel)
    return () => observer.disconnect()
  })
</script>

{#if count && total}
  <p class="count muted small">{count(total)}</p>
{/if}

{#if start > 0}
  <div bind:this={topSentinel} class="sentinel">
    {#if loadingEarlier}<span class="muted small">Loading...</span>{/if}
  </div>
{/if}

{#if total === 0}
  <p class="muted">{empty}</p>
{:else}
  <div
    class="rows"
    class:grid
    bind:this={container}
    style:padding-top="{paddingTop}px"
    style:padding-bottom="{paddingBottom}px"
  >
    {#each shown as entry (key(entry))}
      {@render item(entry)}
    {/each}
  </div>
{/if}

{#if error}
  <p class="error">{error} <button onclick={more}>Retry</button></p>
{/if}
<div bind:this={sentinel} class="sentinel">
  {#if loading}<span class="muted small">Loading...</span>{/if}
</div>

<style>
  .count {
    margin: 4px 0 8px;
  }

  .rows {
    /* Rows come and go while scrolling; the padding keeps everything put. */
    overflow-anchor: none;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 16px 14px;
  }

  .sentinel {
    min-height: 32px;
    padding: 8px 0 16px;
    text-align: center;
  }
</style>
