<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import InfiniteList from '../lib/InfiniteList.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { TrackSort } from '../lib/types'

  type Tab = 'artists' | 'albums' | 'songs'
  const tabs: { id: Tab; label: string }[] = [
    { id: 'artists', label: 'Artists' },
    { id: 'albums', label: 'Albums' },
    { id: 'songs', label: 'Songs' },
  ]

  // Remembered for the session, so Back lands on the same tab.
  let tab = $state((sessionStorage.getItem('djinn-browse-tab') as Tab) || 'artists')
  let sort = $state((sessionStorage.getItem('djinn-browse-sort') as TrackSort) || 'title')
  $effect(() => sessionStorage.setItem('djinn-browse-tab', tab))
  $effect(() => sessionStorage.setItem('djinn-browse-sort', sort))

  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`
</script>

<div class="tabs" role="tablist">
  {#each tabs as t (t.id)}
    <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => (tab = t.id)}>
      {t.label}
    </button>
  {/each}
</div>

{#if tab === 'artists'}
  <InfiniteList
    cacheKey="artists"
    load={api.artists}
    key={(a) => a.id}
    count={(n) => plural(n, 'artist', 'artists')}
    empty="No artists yet. Rescan the library from Admin after adding music."
  >
    {#snippet item(artist)}
      <a class="artist" href="#/artist/{artist.id}">
        <span class="avatar" aria-hidden="true">{artist.name.charAt(0).toUpperCase()}</span>
        <span class="name ellipsis">{artist.name}</span>
        <span class="muted small">{plural(artist.track_count, 'song', 'songs')}</span>
      </a>
    {/snippet}
  </InfiniteList>
{:else if tab === 'albums'}
  <InfiniteList
    cacheKey="albums"
    load={api.albums}
    key={(a) => a.id}
    count={(n) => plural(n, 'album', 'albums')}
    grid
    empty="No albums. Songs without an album tag are under Artists and Songs."
  >
    {#snippet item(album)}
      <a class="album" href="#/album/{album.id}">
        <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
        <span class="ellipsis">{album.title}</span>
        <span class="muted small ellipsis">{album.artist}</span>
      </a>
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
  >
    {#snippet item(track)}
      <TrackRow {track} />
    {/snippet}
  </InfiniteList>
{/if}

<style>
  .tabs {
    display: flex;
    gap: 4px;
    margin: 4px 0 8px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
  }

  .tabs button {
    flex: 1;
    min-height: 40px;
    background: transparent;
    color: var(--muted);
  }

  .tabs button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }

  .artist {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 60px;
    color: var(--text);
    text-decoration: none;
    border-bottom: 1px solid var(--line);
  }

  .avatar {
    flex: none;
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--accent);
    font-weight: 600;
  }

  .name {
    flex: 1;
    min-width: 0;
  }

  .album {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
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
