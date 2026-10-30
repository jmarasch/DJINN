<script lang="ts">
  // Search songs, albums and artists as you type.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import type { SearchResults } from '../../lib/types'
  import SongList from '../SongList.svelte'

  let query = $state(sessionStorageGet())
  let results = $state<SearchResults | null>(null)
  let error = $state('')
  let timer: ReturnType<typeof setTimeout> | undefined

  function sessionStorageGet(): string {
    try {
      return sessionStorage.getItem('go-search') ?? ''
    } catch {
      return ''
    }
  }

  $effect(() => {
    const q = query.trim()
    clearTimeout(timer)
    try {
      sessionStorage.setItem('go-search', query)
    } catch {
      // Private browsing: the search just isn't remembered.
    }
    if (!q) {
      results = null
      return
    }
    timer = setTimeout(async () => {
      try {
        results = await api.search(q)
        error = ''
      } catch (e) {
        error = e instanceof Error ? e.message : 'Search failed'
      }
    }, 250)
  })
</script>

<!-- svelte-ignore a11y_autofocus -->
<input class="search" type="search" placeholder="Songs, artists, albums" bind:value={query} aria-label="Search" autofocus />
{#if error}<p class="error">{error}</p>{/if}
{#if results}
  {#if results.artists.length}
    <h2>Artists</h2>
    <div class="tiles">
      {#each results.artists as artist (artist.id)}
        <a class="tile" href="#/artist/{artist.id}">
          <Art hash={artist.art_hash} size={88} round={999} letter={artist.name[0]} />
          <span class="small">{artist.name}</span>
        </a>
      {/each}
    </div>
  {/if}
  {#if results.albums.length}
    <h2>Albums</h2>
    <div class="tiles">
      {#each results.albums as album (album.id)}
        <a class="tile" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={88} />
          <span class="small">{album.title}</span>
          <span class="muted small">{album.artist}</span>
        </a>
      {/each}
    </div>
  {/if}
  <h2>Songs</h2>
  {#if results.tracks.length}
    <SongList tracks={results.tracks} />
  {:else}
    <p class="muted">No songs match.</p>
  {/if}
{/if}

<style>
  .search {
    width: 100%;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .tiles {
    display: flex;
    gap: 12px;
    overflow-x: auto;
    padding-bottom: 4px;
  }

  .tile {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 4px;
    width: 88px;
    color: inherit;
    text-decoration: none;
  }

  .tile span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
