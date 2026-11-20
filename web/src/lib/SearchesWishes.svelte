<script lang="ts">
  // Admin > Stats: what people searched for (nothing found is a shopping
  // list) and the songs they asked for with "Not in the library?".
  import { api } from './api'
  import { clearListCache } from './listCache'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
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
      toast(e instanceof Error ? e.message : t('sw.load_failed'), true)
    }
  }

  async function loadWishes() {
    try {
      wishes = await api.wishes(wishStatus)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sw.wishes_failed'), true)
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
      toast(e instanceof Error ? e.message : t('sw.save_failed'), true)
    }
  }

  async function clearSearches() {
    if (!confirm(t('sw.clear_q'))) return
    try {
      await api.clearSearchHistory()
      searches = []
    } catch (e) {
      toast(e instanceof Error ? e.message : t('st.clear_failed'), true)
    }
  }

  async function mark(wish: Wish, status: Wish['status']) {
    try {
      await api.setWish(wish.id, status)
      wishes = wishes.filter((w) => w !== wish)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sw.save_failed'), true)
    }
  }

  async function remove(wish: Wish) {
    try {
      await api.deleteWish(wish.id)
      wishes = wishes.filter((w) => w !== wish)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.remove_failed'), true)
    }
  }

  const n = (x: number) => x.toLocaleString(locale.code)
  const day = (ms: number) => new Date(ms).toLocaleDateString(locale.code, { month: 'short', day: 'numeric' })
</script>

<section class="card">
  <h2>{t('sw.title')}</h2>
  {#if settings}
    <label class="switch big">
      <input type="checkbox" bind:checked={settings.search_history} onchange={saveSetting} />
      {t('sw.keep')}
    </label>
  {/if}
  <div class="chips" role="group" aria-label={t('sw.which')}>
    <button class:active={nothingFound} aria-pressed={nothingFound} onclick={() => (nothingFound = true)}>{t('sw.found_nothing')}</button>
    <button class:active={!nothingFound} aria-pressed={!nothingFound} onclick={() => (nothingFound = false)}>{t('sw.all')}</button>
  </div>
  <p class="small muted">
    {nothingFound
      ? t('sw.nothing_note')
      : t('sw.most_note')}
  </p>
  {#each searches as s (s.query)}
    <div class="line">
      <span class="grow">{s.query}</span>
      <span class="small muted">
        {n(s.searches)}×{#if s.people > 1}{t('sw.people', { n: n(s.people) })}{/if}{#if !nothingFound}{t('sw.found', {
            n: n(s.results),
          })}{/if}{#if s.requested}{t('sw.requested', { n: n(s.requested) })}{/if}, {day(s.last_at)}
      </span>
    </div>
  {:else}
    <p class="small muted">{t('sw.none')}</p>
  {/each}
  {#if searches.length}<button class="small-button" onclick={clearSearches}>{t('sw.clear')}</button>{/if}
</section>

<section class="card">
  <h2>{t('sw.wishlist')}</h2>
  <p class="small muted">
    {t('sw.wishlist_note')}
  </p>
  {#if settings}
    <label class="switch big">
      <input type="checkbox" bind:checked={settings.wishlist_everyone} onchange={saveSetting} />
      {t('sw.everyone')}
    </label>
  {/if}
  <div class="chips" role="group" aria-label={t('sw.which_wishes')}>
    {#each [['open', t('sw.wanted')], ['got', t('sw.got')], ['ignored', t('sw.ignored')]] as [id, label] (id)}
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
      <span class="small muted">{t('sw.n_people', { n: wish.askers })}</span>
      {#if wish.status !== 'got'}<button class="small-button" onclick={() => mark(wish, 'got')}>{t('sw.got_it')}</button>{/if}
      {#if wish.status === 'open'}
        <button class="small-button" onclick={() => mark(wish, 'ignored')}>{t('sw.ignore')}</button>
      {:else}
        <button class="small-button" onclick={() => remove(wish)}>{t('common.remove')}</button>
      {/if}
    </div>
  {:else}
    <p class="small muted">{t('sw.nothing_here')}</p>
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
