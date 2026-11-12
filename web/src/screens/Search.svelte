<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import ScrollRow from '../lib/ScrollRow.svelte'
  import Browse from './Browse.svelte'
  import PageTitle from '../lib/PageTitle.svelte'
  import Icon from '../lib/Icon.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import NotInLibrary from '../lib/NotInLibrary.svelte'
  import { isAdmin } from '../lib/state.svelte'
  import SearchSongActions from '../lib/SearchSongActions.svelte'
  import type { SearchResults } from '../lib/types'

  let searchBox = $state<HTMLElement>()

  // Remember the last search when coming back from an album or artist.
  let query = $state(sessionStorage.getItem('djinn-search') ?? '')
  let input = $state<HTMLInputElement>()
  let results = $state<SearchResults | null>(null)
  let loading = $state(false)
  /** The AI reader is reading the query. */
  let thinking = $state(false)
  let latest = 0

  const musings = [
    'Thinking...',
    'Digging through the crates...',
    'Flipping through records...',
    'Asking the DJ...',
    'Reading the room...',
    'Checking the back catalog...',
    'Cueing up ideas...',
  ]
  let musing = $state(musings[0])

  // A new phrase every couple of seconds while the reader works.
  $effect(() => {
    if (!thinking) return
    let at = Math.floor(Math.random() * musings.length)
    musing = musings[at]
    const timer = setInterval(() => {
      at = (at + 1 + Math.floor(Math.random() * (musings.length - 1))) % musings.length
      musing = musings[at]
    }, 2200)
    return () => clearInterval(timer)
  })

  $effect(() => {
    const q = query.trim()
    sessionStorage.setItem('djinn-search', query)
    if (!q) {
      results = null
      return
    }
    const id = ++latest
    thinking = false
    const timer = setTimeout(async () => {
      loading = true
      try {
        const found = await api.search(q)
        if (id !== latest) return
        results = found
        loading = false
        if (!found.ask) return
        // Words the phrase reader didn't know: once typing stops, let the
        // AI reader have a go (a few seconds).
        thinking = true
        await new Promise((resolve) => setTimeout(resolve, 600))
        if (id !== latest) return
        const read = await api.ask(q)
        if (id === latest && read && results) results = { ...results, described: read }
      } catch {
        // Keep what was found.
      } finally {
        if (id === latest) {
          loading = false
          thinking = false
        }
      }
    }, 150)
    return () => clearTimeout(timer)
  })

  const described = $derived(results?.described ?? null)
  const empty = $derived(
    results &&
      !results.tracks.length &&
      !results.albums.length &&
      !results.artists.length &&
      !results.lyrics?.length &&
      !described?.tracks.length,
  )

  /** Plain matches by name come first when there are any; a description
   * with nothing by that name leads with what fits. */
  const describedFirst = $derived(!!described && !results?.tracks.length && !results?.artists.length)

</script>

{#snippet describedSection()}
  {#if thinking}
    <p class="small muted thinking"><span class="spinner" aria-hidden="true"></span> {musing}</p>
  {/if}
  {#if described}
    <div class="described">
      <div class="described-head">
        <div>
          <h2>
            {t('search.matches')} <span class="muted">{described.summary.join(' · ')}</span>
            {#if described.read_by_ai}<span class="tag">{t('search.read_by_ai')}</span>{/if}
          </h2>
          {#if described.tracks.length}
            <p class="small muted">
              {t('search.fit', { n: described.total })}{described.total > described.tracks.length
                ? t('search.showing', { n: described.tracks.length })
                : ''}.{described.unknown.length ? ` ${t('search.not_sure', { words: described.unknown.join(' ') })}` : ''}
            </p>
          {:else}
            <p class="small muted">
              {t('search.nothing_fits')}{#if isAdmin()}
                Genres come from your files' tags and from MusicBrainz (Admin > Rules), so songs without either can't
                match a genre.{/if}
            </p>
          {/if}
        </div>
      </div>
      <SearchSongActions name={described.summary.join(' ')} tracks={described.tracks} />
      {#if described.artists.length}
        <div class="chips">
          {#each described.artists as artist (artist.id)}
            <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
          {/each}
        </div>
      {/if}
      {#each described.tracks as track (track.id)}
        <TrackRow {track} />
      {/each}
    </div>
  {/if}
{/snippet}

<PageTitle title={t('nav.library')} subtitle={t('library.subtitle')} />

<div class="search-box" bind:this={searchBox}>
  <Icon name="search" />
  <div class="field">
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="search"
            placeholder={t('search.placeholder')}
      bind:value={query}
      bind:this={input}
      autofocus
      autocomplete="off"
      autocapitalize="none"
      enterkeyhint="search"
    />
    {#if query}
      <button
        class="clear"
        aria-label={t('search.clear')}
        onclick={() => {
          query = ''
          input?.focus()
        }}
      >
        <Icon name="close" size={20} />
      </button>
    {/if}
  </div>
</div>

{#if empty && !loading && !thinking && !described}
  <div class="nothing">
    <img src="/images/nothing-found.webp" alt="" />
    <p class="muted">{t('search.no_matches', { query: query.trim() })}</p>
  </div>
{/if}

{#if results}
  {#if describedFirst}{@render describedSection()}{/if}
  {#if results.artists.length}
    <h2>{t('library.artists')}</h2>
    <div class="chips">
      {#each results.artists as artist (artist.id)}
        <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
      {/each}
    </div>
  {/if}

  {#if results.albums.length}
    <h2>{t('library.albums')}</h2>
    <ScrollRow>
      {#each results.albums as album (album.id)}
        <a class="album" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={120} round={10} />
          <span class="ellipsis">{album.title}</span>
          <span class="muted small ellipsis">{album.artist}</span>
        </a>
      {/each}
    </ScrollRow>
  {/if}

  {#if results.tracks.length}
    <h2>{t('library.songs')}</h2>
    <SearchSongActions name={query} tracks={results.tracks} />
    {#each results.tracks as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
  {#if results.lyrics?.length}
    <h2>{t('search.lyric_matches')}</h2>
    {#each results.lyrics as hit (hit.track.id)}
      <TrackRow track={hit.track} />
      <p class="quote">“{hit.line}”</p>
    {/each}
  {/if}
  {#if !describedFirst}{@render describedSection()}{/if}
  <NotInLibrary {query} />
{:else if !query.trim()}
  <Browse stuckBelow={searchBox} />
{/if}

<style>
  .nothing {
    display: flex;
    flex-direction: column;
    align-items: center;
    margin-top: 24px;
    text-align: center;
  }

  .nothing img {
    width: min(170px, 50vw);
    height: auto;
  }

  /* The lyric line a search matched, under its song. */
  .quote {
    margin: -4px 0 10px 56px;
    color: var(--muted);
    font-size: 0.875rem;
    font-style: italic;
  }

  .thinking {
    margin: 12px 0;
  }

  .tag {
    margin-left: 6px;
    padding: 1px 6px;
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 0.6875rem;
    font-weight: 500;
    color: var(--muted);
    vertical-align: 2px;
  }

  .described-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }

  .described-head h2 {
    margin-bottom: 2px;
  }

  .described-head p {
    margin: 0 0 8px;
  }

  .search-box {
    position: sticky;
    top: calc(var(--nav-height, 0px) + 8px);
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 0 12px;
    padding: 2px 6px 2px 16px;
    border: 1px solid var(--edge);
    border-radius: 16px;
    background: var(--surface-2);
    box-shadow: var(--glow);
    color: var(--muted);
  }

  .search-box:focus-within {
    border-color: var(--edge-strong);
    box-shadow: var(--glow-strong);
  }

  .search-box :global(input),
  .search-box :global(input:focus-visible) {
    border: 0;
    background: transparent;
    box-shadow: none;
  }

  .field {
    position: relative;
    flex: 1;
    display: flex;
  }

  .search-box input {
    flex: 1;
    padding-right: 44px;
    font-size: 1.0625rem;
  }

  /* Our own clear button instead of the browser's, which not every browser
     shows. */
  .search-box input::-webkit-search-cancel-button {
    appearance: none;
  }

  .clear {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 44px;
    min-height: 0;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .clear:hover {
    color: var(--text);
  }

  .album {
    flex: none;
    width: 120px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }
</style>
