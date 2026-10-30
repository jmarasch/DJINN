<script lang="ts">
  // The whole library: search as you type, or browse by artist, album,
  // song or genre (the same browser as the party app). Tapping a song in
  // the big lists plays it, then songs like it.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import type { SearchResults, Track } from '../../lib/types'
  import Browse from '../../screens/Browse.svelte'
  import { startRadio } from '../player.svelte'
  import SongList from '../SongList.svelte'
  import SongRow from '../SongRow.svelte'
  import SongTile from '../SongTile.svelte'

  let query = $state(remembered())
  let results = $state<SearchResults | null>(null)
  let error = $state('')
  let timer: ReturnType<typeof setTimeout> | undefined

  function remembered(): string {
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

  const play = (track: Track) => startRadio(track)
</script>

<div class="search-bar">
  <input class="search" type="search" placeholder="Search songs, artists, albums" bind:value={query} aria-label="Search" />
</div>
{#if error}<p class="error">{error}</p>{/if}

{#if !query.trim()}
  <Browse progress={false}>
    {#snippet song(track)}
      <SongRow {track} onplay={() => play(track)} />
    {/snippet}
    {#snippet songTile(track)}
      <SongTile {track} onplay={() => play(track)} />
    {/snippet}
  </Browse>
{:else if results}
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
  .search-bar {
    position: sticky;
    top: 60px;
    z-index: 5;
    padding: 4px 0 8px;
    background: var(--bg);
  }

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
