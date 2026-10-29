<script lang="ts" generics="T">
  import { tick, untrack, type Snippet } from 'svelte'
  import { listCache } from './listCache'
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

  let items = $state<T[]>([])
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
      const first = container?.firstElementChild
      const before = first?.getBoundingClientRect().top
      items = [...page.items.slice(0, start - from), ...items]
      start = from
      remember()
      await tick()
      // Browsers that anchor scrolling have already kept it in place; this
      // only moves the difference that's left.
      if (first && before !== undefined) scrollBy(0, first.getBoundingClientRect().top - before)
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
    const entry = container?.children[index]
    if (entry) scrollTo({ top: entry.getBoundingClientRect().top + scrollY - inset })
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
  <div class:grid bind:this={container}>
    {#each items as entry (key(entry))}
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
