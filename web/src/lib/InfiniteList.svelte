<script lang="ts" generics="T">
  import { untrack, type Snippet } from 'svelte'
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
  } = $props()

  /** Starts fetching the next page this far before the end scrolls into view. */
  const PRELOAD_PX = 800

  let items = $state<T[]>([])
  let total = $state<number | null>(null)
  let loading = $state(false)
  let error = $state('')
  let sentinel = $state<HTMLElement>()
  let generation = 0

  $effect(() => {
    const k = cacheKey
    untrack(() => {
      generation++
      loading = false
      error = ''
      const cached = listCache.get(k)
      if (cached) {
        items = cached.items as T[]
        total = cached.total
      } else {
        items = []
        total = null
        more()
      }
    })
  })

  async function more() {
    if (loading || (total !== null && items.length >= total)) return
    loading = true
    const gen = generation
    const k = cacheKey
    try {
      const page = await load(items.length)
      if (gen !== generation) return
      items = [...items, ...page.items]
      // An empty page means the library shrank under us; stop here.
      total = page.items.length ? page.total : items.length
      listCache.set(k, { items, total })
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
</script>

{#if count && total}
  <p class="count muted small">{count(total)}</p>
{/if}

{#if total === 0}
  <p class="muted">{empty}</p>
{:else}
  <div class:grid>
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
