<script lang="ts">
  // Your favorites: the artists, albums and songs you've hearted.
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Heart from '../lib/Heart.svelte'
  import ScrollRow from '../lib/ScrollRow.svelte'
  import { toast } from '../lib/state.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { Liked } from '../lib/types'

  let liked = $state<Liked | null>(null)

  $effect(() => {
    api
      .liked()
      .then((l) => (liked = l))
      .catch((e) => toast(e instanceof Error ? e.message : 'Could not load your favorites', true))
  })

  const empty = $derived(liked && !liked.artists.length && !liked.albums.length && !liked.tracks.length)
</script>

<h1>Favorites</h1>
{#if !liked}
  <p class="muted small">Loading...</p>
{:else if empty}
  <div class="empty">
    <img src="/images/no-favorites.webp" alt="" />
    <p class="muted">Tap the heart on a song, album or artist to keep it here.</p>
  </div>
{:else}
  {#if liked.artists.length}
    <h2>Artists</h2>
    <ScrollRow gap={12}>
      {#each liked.artists as artist (artist.id)}
        <div class="tile">
          <a href="#/artist/{artist.id}">
            <Art hash={artist.art_hash} size={96} round={999} letter={artist.name[0]} />
            <span class="small ellipsis">{artist.name}</span>
          </a>
          <div class="corner"><Heart kind="artist" id={artist.id} name={artist.name} size={18} /></div>
        </div>
      {/each}
    </ScrollRow>
  {/if}
  {#if liked.albums.length}
    <h2>Albums</h2>
    <ScrollRow gap={12}>
      {#each liked.albums as album (album.id)}
        <div class="tile">
          <a href="#/album/{album.id}">
            <Art hash={album.art_hash} size={96} />
            <span class="small ellipsis">{album.title}</span>
            <span class="muted small ellipsis">{album.artist}</span>
          </a>
          <div class="corner"><Heart kind="album" id={album.id} name={album.title} size={18} /></div>
        </div>
      {/each}
    </ScrollRow>
  {/if}
  {#if liked.tracks.length}
    <h2>Songs</h2>
    {#each liked.tracks as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
{/if}

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .empty img {
    width: min(190px, 55vw);
    height: auto;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .tile {
    position: relative;
    flex: none;
    width: 96px;
  }

  .tile a {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: inherit;
    text-decoration: none;
  }

  .corner {
    position: absolute;
    top: 0;
    right: 0;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.45);
  }

  .corner :global(.heart:not(.liked)) {
    color: #fff;
  }
</style>
