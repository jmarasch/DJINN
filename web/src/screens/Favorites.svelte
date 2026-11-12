<script lang="ts">
  import PageTitle from '../lib/PageTitle.svelte'
  import { t } from '../lib/i18n.svelte'
  // Your favorites: the artists, albums and songs you've hearted.
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Heart from '../lib/Heart.svelte'
  import ScrollRow from '../lib/ScrollRow.svelte'
  import { toast } from '../lib/state.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { Liked } from '../lib/types'

  let liked = $state<Liked | null>(null)
  // A panel shows a few and opens to everything with See all.
  const FEW = 8
  const FEW_SONGS = 5
  let all = $state({ artists: false, albums: false, tracks: false })

  $effect(() => {
    api
      .liked()
      .then((l) => (liked = l))
      .catch((e) => toast(e instanceof Error ? e.message : 'Could not load your favorites', true))
  })

  const empty = $derived(liked && !liked.artists.length && !liked.albums.length && !liked.tracks.length)
</script>

<PageTitle title={t('nav.favorites')} subtitle={t('favorites.subtitle')} />
{#snippet artistTile(artist: Liked['artists'][number])}
  <div class="tile">
    <a href="#/artist/{artist.id}">
      <Art hash={artist.art_hash} size={96} round={999} letter={artist.name[0]} />
      <span class="small ellipsis">{artist.name}</span>
    </a>
    <div class="corner"><Heart kind="artist" id={artist.id} name={artist.name} size={18} /></div>
  </div>
{/snippet}
{#snippet albumTile(album: Liked['albums'][number])}
  <div class="tile">
    <a href="#/album/{album.id}">
      <Art hash={album.art_hash} size={96} />
      <span class="small ellipsis">{album.title}</span>
      <span class="muted small ellipsis">{album.artist}</span>
    </a>
    <div class="corner"><Heart kind="album" id={album.id} name={album.title} size={18} /></div>
  </div>
{/snippet}
{#if !liked}
  <p class="muted small">Loading...</p>
{:else if empty}
  <div class="empty">
    <img src="/images/no-favorites.webp" alt="" />
    <p class="muted">Tap the heart on a song, album or artist to keep it here.</p>
  </div>
{:else}
  {#if liked.artists.length}
    <section class="panel">
      <div class="head">
        <h2>Artists</h2>
        {#if liked.artists.length > FEW}
          <button class="see-all" aria-expanded={all.artists} onclick={() => (all.artists = !all.artists)}>
            {all.artists ? t('favorites.fewer') : t('favorites.see_all')}
          </button>
        {/if}
      </div>
      {#if all.artists}
        <div class="wrap">{#each liked.artists as artist (artist.id)}{@render artistTile(artist)}{/each}</div>
      {:else}
        <ScrollRow gap={12}>{#each liked.artists.slice(0, FEW) as artist (artist.id)}{@render artistTile(artist)}{/each}</ScrollRow>
      {/if}
    </section>
  {/if}
  {#if liked.albums.length}
    <section class="panel">
      <div class="head">
        <h2>Albums</h2>
        {#if liked.albums.length > FEW}
          <button class="see-all" aria-expanded={all.albums} onclick={() => (all.albums = !all.albums)}>
            {all.albums ? t('favorites.fewer') : t('favorites.see_all')}
          </button>
        {/if}
      </div>
      {#if all.albums}
        <div class="wrap">{#each liked.albums as album (album.id)}{@render albumTile(album)}{/each}</div>
      {:else}
        <ScrollRow gap={12}>{#each liked.albums.slice(0, FEW) as album (album.id)}{@render albumTile(album)}{/each}</ScrollRow>
      {/if}
    </section>
  {/if}
  {#if liked.tracks.length}
    <section class="panel">
      <div class="head">
        <h2>Songs</h2>
        {#if liked.tracks.length > FEW_SONGS}
          <button class="see-all" aria-expanded={all.tracks} onclick={() => (all.tracks = !all.tracks)}>
            {all.tracks ? t('favorites.fewer') : t('favorites.see_all')}
          </button>
        {/if}
      </div>
      {#each all.tracks ? liked.tracks : liked.tracks.slice(0, FEW_SONGS) as track (track.id)}
        <TrackRow {track} />
      {/each}
    </section>
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

  .panel {
    margin: 14px 0;
    padding: 14px 16px;
    border: 1px solid var(--edge);
    border-radius: 18px;
    background: var(--card-bg);
    box-shadow: var(--glow);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 8px;
  }

  h2 {
    margin: 0;
    font-size: 1.05rem;
  }

  .see-all {
    min-height: 32px;
    padding: 0 12px;
    border: 1px solid var(--edge);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--accent);
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .wrap {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
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
