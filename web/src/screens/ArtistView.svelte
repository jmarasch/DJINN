<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import PoolButton from '../lib/PoolButton.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { ArtistDetail } from '../lib/types'

  let { id }: { id: number } = $props()
  let artist = $state<ArtistDetail | null>(null)
  let error = $state('')

  $effect(() => {
    artist = null
    api
      .artist(id)
      .then((a) => (artist = a))
      .catch((e) => (error = e.message))
  })
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

{#if error}
  <p class="error">{error}</p>
{:else if artist}
  <h1>{artist.name}</h1>
  <PoolButton kind="artist" refId={artist.id} />

  {#if artist.albums.length}
    <h2>Albums</h2>
    {#each artist.albums as album (album.id)}
      <a class="album" href="#/album/{album.id}">
        <Art hash={album.art_hash} size={56} />
        <div>
          <div>{album.title}</div>
          {#if album.year}<div class="muted small">{album.year}</div>{/if}
        </div>
      </a>
    {/each}
  {/if}

  {#if artist.other_tracks.length}
    <h2>{artist.albums.length ? 'Other songs' : 'Songs'}</h2>
    {#each artist.other_tracks as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
{/if}

<style>
  h1 {
    margin: 4px 0 8px;
  }

  .album {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 64px;
    color: var(--text);
    text-decoration: none;
  }
</style>
