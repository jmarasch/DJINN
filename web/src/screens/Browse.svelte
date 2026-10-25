<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import InfiniteList from '../lib/InfiniteList.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import TrackTile from '../lib/TrackTile.svelte'
  import type { TrackSort } from '../lib/types'

  type Tab = 'artists' | 'albums' | 'songs'
  type View = 'list' | 'grid'
  const tabs: { id: Tab; label: string }[] = [
    { id: 'artists', label: 'Artists' },
    { id: 'albums', label: 'Albums' },
    { id: 'songs', label: 'Songs' },
  ]

  // Tab and sort last for the session; the list/grid choice is kept on
  // this device.
  let tab = $state((sessionStorage.getItem('djinn-browse-tab') as Tab) || 'artists')
  let sort = $state((sessionStorage.getItem('djinn-browse-sort') as TrackSort) || 'title')
  let view = $state<View>(readView())
  $effect(() => sessionStorage.setItem('djinn-browse-tab', tab))
  $effect(() => sessionStorage.setItem('djinn-browse-sort', sort))
  $effect(() => {
    try {
      localStorage.setItem('djinn-browse-view', view)
    } catch {
      // Storage unavailable (private mode); the choice just isn't kept.
    }
  })

  function readView(): View {
    try {
      return localStorage.getItem('djinn-browse-view') === 'grid' ? 'grid' : 'list'
    } catch {
      return 'list'
    }
  }

  const grid = $derived(view === 'grid')
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`
  const initial = (name: string) => name.charAt(0).toUpperCase()
</script>

<div class="bar">
  <div class="tabs" role="tablist">
    {#each tabs as t (t.id)}
      <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => (tab = t.id)}>
        {t.label}
      </button>
    {/each}
  </div>
  <div class="views" role="group" aria-label="Layout">
    <button class:active={!grid} aria-label="List" aria-pressed={!grid} onclick={() => (view = 'list')}>
      <Icon name="list" size={20} />
    </button>
    <button class:active={grid} aria-label="Grid" aria-pressed={grid} onclick={() => (view = 'grid')}>
      <Icon name="grid" size={20} />
    </button>
  </div>
</div>

{#if tab === 'artists'}
  <InfiniteList
    cacheKey="artists"
    load={api.artists}
    key={(a) => a.id}
    count={(n) => plural(n, 'artist', 'artists')}
    {grid}
    empty="No artists yet. Rescan the library from Admin after adding music."
  >
    {#snippet item(artist)}
      {#if grid}
        <a class="tile" href="#/artist/{artist.id}">
          <div class="cover"><Art hash={artist.art_hash} size={160} round={999} letter={initial(artist.name)} /></div>
          <span class="ellipsis center-text">{artist.name}</span>
          <span class="muted small center-text">{plural(artist.track_count, 'song', 'songs')}</span>
        </a>
      {:else}
        <a class="row" href="#/artist/{artist.id}">
          <Art hash={artist.art_hash} size={44} round={999} letter={initial(artist.name)} />
          <span class="name ellipsis">{artist.name}</span>
          <span class="muted small">{plural(artist.track_count, 'song', 'songs')}</span>
        </a>
      {/if}
    {/snippet}
  </InfiniteList>
{:else if tab === 'albums'}
  <InfiniteList
    cacheKey="albums"
    load={api.albums}
    key={(a) => a.id}
    count={(n) => plural(n, 'album', 'albums')}
    {grid}
    empty="No albums. Songs without an album tag are under Artists and Songs."
  >
    {#snippet item(album)}
      {#if grid}
        <a class="tile" href="#/album/{album.id}">
          <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
          <span class="ellipsis">{album.title}</span>
          <span class="muted small ellipsis">{album.artist}</span>
        </a>
      {:else}
        <a class="row" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={52} />
          <span class="name">
            <span class="ellipsis block">{album.title}</span>
            <span class="muted small ellipsis block">
              {album.artist}{album.year ? ` · ${album.year}` : ''}
            </span>
          </span>
        </a>
      {/if}
    {/snippet}
  </InfiniteList>
{:else}
  <label class="sort">
    <span class="muted small">Sort by</span>
    <select bind:value={sort}>
      <option value="title">Title</option>
      <option value="artist">Artist</option>
      <option value="added">Recently added</option>
    </select>
  </label>
  <InfiniteList
    cacheKey="songs-{sort}"
    load={(offset) => api.tracks(sort, offset)}
    key={(t) => t.id}
    count={(n) => plural(n, 'song', 'songs')}
    {grid}
  >
    {#snippet item(track)}
      {#if grid}
        <TrackTile {track} />
      {:else}
        <TrackRow {track} />
      {/if}
    {/snippet}
  </InfiniteList>
{/if}

<style>
  .bar {
    display: flex;
    gap: 8px;
    margin: 4px 0 8px;
  }

  .tabs,
  .views {
    display: flex;
    gap: 4px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
  }

  .tabs {
    flex: 1;
  }

  .tabs button,
  .views button {
    min-height: 40px;
    background: transparent;
    color: var(--muted);
  }

  .tabs button {
    flex: 1;
    padding: 0 6px;
  }

  .views button {
    width: 40px;
    padding: 0;
  }

  .tabs button.active,
  .views button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 62px;
    color: var(--text);
    text-decoration: none;
    border-bottom: 1px solid var(--line);
  }

  .name {
    flex: 1;
    min-width: 0;
  }

  .block {
    display: block;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }

  .center-text {
    text-align: center;
  }

  .cover {
    aspect-ratio: 1;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  .sort {
    flex-direction: row;
    align-items: center;
    gap: 10px;
    margin-bottom: 4px;
  }
</style>
