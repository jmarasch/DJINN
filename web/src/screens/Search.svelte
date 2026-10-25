<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Browse from './Browse.svelte'
  import Icon from '../lib/Icon.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { SearchResults } from '../lib/types'

  // Remember the last search when coming back from an album or artist.
  let query = $state(sessionStorage.getItem('djinn-search') ?? '')
  let results = $state<SearchResults | null>(null)
  let loading = $state(false)
  let latest = 0

  $effect(() => {
    const q = query.trim()
    sessionStorage.setItem('djinn-search', query)
    if (!q) {
      results = null
      return
    }
    const id = ++latest
    const timer = setTimeout(async () => {
      loading = true
      try {
        const found = await api.search(q)
        if (id === latest) results = found
      } finally {
        if (id === latest) loading = false
      }
    }, 150)
    return () => clearTimeout(timer)
  })

  const empty = $derived(
    results && !results.tracks.length && !results.albums.length && !results.artists.length,
  )
</script>

<div class="search-box">
  <Icon name="search" />
  <!-- svelte-ignore a11y_autofocus -->
  <input
    type="search"
    placeholder="Search songs, artists, albums"
    bind:value={query}
    autofocus
    autocomplete="off"
    autocapitalize="none"
    enterkeyhint="search"
  />
</div>

{#if empty && !loading}
  <p class="muted center">No matches for "{query.trim()}".</p>
{/if}

{#if results}
  {#if results.artists.length}
    <h2>Artists</h2>
    <div class="chips">
      {#each results.artists as artist (artist.id)}
        <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
      {/each}
    </div>
  {/if}

  {#if results.albums.length}
    <h2>Albums</h2>
    <div class="albums">
      {#each results.albums as album (album.id)}
        <a class="album" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={120} round={10} />
          <span class="ellipsis">{album.title}</span>
          <span class="muted small ellipsis">{album.artist}</span>
        </a>
      {/each}
    </div>
  {/if}

  {#if results.tracks.length}
    <h2>Songs</h2>
    {#each results.tracks as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
{:else if !query.trim()}
  <Browse />
{/if}

<style>
  .search-box {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 -16px;
    padding: 12px 16px;
    background: var(--bg);
    color: var(--muted);
  }

  .search-box input {
    flex: 1;
    font-size: 1.0625rem;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .chip {
    padding: 10px 16px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text);
    text-decoration: none;
  }

  .albums {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 120px;
    gap: 14px;
    overflow-x: auto;
    margin: 0 -16px;
    padding: 0 16px 6px;
    scrollbar-width: none;
  }

  .album {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }
</style>
