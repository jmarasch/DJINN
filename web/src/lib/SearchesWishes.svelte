<script lang="ts">
  // Admin > Stats: what people searched for (nothing found is a shopping
  // list) and the songs they asked for with "Not in the library?".
  import { api } from './api'
  import { clearListCache } from './listCache'
  import { toast } from './state.svelte'
  import type { SearchCount, Settings, Wish } from './types'

  let searches = $state<SearchCount[]>([])
  let nothingFound = $state(true)
  let wishStatus = $state<Wish['status']>('open')
  let wishes = $state<Wish[]>([])
  let settings = $state<Settings | null>(null)

  async function loadSearches() {
    try {
      searches = (await api.searchHistory(Date.now() - 90 * 86_400_000, nothingFound)).searches
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load searches', true)
    }
  }

  async function loadWishes() {
    try {
      wishes = await api.wishes(wishStatus)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the wishlist', true)
    }
  }

  $effect(() => {
    void nothingFound
    void loadSearches()
  })
  $effect(() => {
    void wishStatus
    void loadWishes()
  })
  $effect(() => {
    api.settings().then((s) => (settings = s)).catch(() => {})
  })

  async function saveSetting() {
    if (!settings) return
    try {
      settings = await api.saveSettings(settings)
      clearListCache()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save that', true)
    }
  }

  async function clearSearches() {
    if (!confirm('Clear the search history?')) return
    try {
      await api.clearSearchHistory()
      searches = []
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not clear it', true)
    }
  }

  async function mark(wish: Wish, status: Wish['status']) {
    try {
      await api.setWish(wish.id, status)
      wishes = wishes.filter((w) => w !== wish)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save that', true)
    }
  }

  async function remove(wish: Wish) {
    try {
      await api.deleteWish(wish.id)
      wishes = wishes.filter((w) => w !== wish)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove it', true)
    }
  }

  const n = (x: number) => x.toLocaleString()
  const day = (ms: number) => new Date(ms).toLocaleDateString([], { month: 'short', day: 'numeric' })
</script>

<section class="card">
  <h2>Searches</h2>
  {#if settings}
    <label class="switch big">
      <input type="checkbox" bind:checked={settings.search_history} onchange={saveSetting} />
      Keep a history of searches
    </label>
  {/if}
  <div class="chips" role="group" aria-label="Which searches">
    <button class:active={nothingFound} aria-pressed={nothingFound} onclick={() => (nothingFound = true)}>Found nothing</button>
    <button class:active={!nothingFound} aria-pressed={!nothingFound} onclick={() => (nothingFound = false)}>All searches</button>
  </div>
  <p class="small muted">
    {nothingFound
      ? 'Searches in the last 90 days that found nothing: songs people want that the library lacks.'
      : 'Searched most in the last 90 days.'}
  </p>
  {#each searches as s (s.query)}
    <div class="line">
      <span class="grow">{s.query}</span>
      <span class="small muted">
        {n(s.searches)}×{#if s.people > 1}, {n(s.people)} people{/if}{#if !nothingFound}, {n(s.results)} found{/if}{#if s.requested}, {n(s.requested)} requested{/if}, {day(s.last_at)}
      </span>
    </div>
  {:else}
    <p class="small muted">None yet.</p>
  {/each}
  {#if searches.length}<button class="small-button" onclick={clearSearches}>Clear the search history</button>{/if}
</section>

<section class="card">
  <h2>Wishlist</h2>
  <p class="small muted">
    Songs people asked for with "Not in the library?" under search results. When a library check adds one, it moves to Got.
  </p>
  {#if settings}
    <label class="switch big">
      <input type="checkbox" bind:checked={settings.wishlist_everyone} onchange={saveSetting} />
      Let everyone ask for songs that are not in the library (otherwise only admins see the button)
    </label>
  {/if}
  <div class="chips" role="group" aria-label="Which wishes">
    {#each [['open', 'Wanted'], ['got', 'Got'], ['ignored', 'Ignored']] as [id, label] (id)}
      <button
        class:active={wishStatus === id}
        aria-pressed={wishStatus === id}
        onclick={() => (wishStatus = id as Wish['status'])}>{label}</button
      >
    {/each}
  </div>
  {#each wishes as wish (wish.id)}
    <div class="line">
      <span class="grow">{wish.title} <span class="muted">· {wish.artist}</span></span>
      <span class="small muted">{n(wish.askers)} {wish.askers === 1 ? 'person' : 'people'}</span>
      {#if wish.status !== 'got'}<button class="small-button" onclick={() => mark(wish, 'got')}>Got it</button>{/if}
      {#if wish.status === 'open'}
        <button class="small-button" onclick={() => mark(wish, 'ignored')}>Ignore</button>
      {:else}
        <button class="small-button" onclick={() => remove(wish)}>Remove</button>
      {/if}
    </div>
  {:else}
    <p class="small muted">Nothing here.</p>
  {/each}
</section>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chips button {
    min-height: 36px;
    padding: 0 12px;
    border-radius: 999px;
    font-size: 0.875rem;
  }

  .chips button.active {
    background: var(--accent);
    color: var(--accent-text);
  }

  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 140px;
    overflow-wrap: anywhere;
  }
</style>
