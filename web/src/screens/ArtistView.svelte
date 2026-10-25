<script lang="ts">
  import AddToPlaylist from '../lib/AddToPlaylist.svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { ArtistDetail } from '../lib/types'

  let { id }: { id: number } = $props()
  let artist = $state<ArtistDetail | null>(null)
  let error = $state('')
  let bioOpen = $state(false)

  $effect(() => {
    artist = null
    bioOpen = false
    api
      .artist(id)
      .then((a) => (artist = a))
      .catch((e) => (error = e.message))
  })

  /** Top songs earn the title once anyone has requested or played them. */
  const ranked = $derived(artist?.top_tracks.some((t) => t.requests || t.plays) ?? false)
  const topIds = $derived(new Set(artist?.top_tracks.map((t) => t.id)))
  const moreSongs = $derived(artist?.other_tracks.filter((t) => !topIds.has(t.id)) ?? [])
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

{#if error}
  <p class="error">{error}</p>
{:else if artist}
  <header>
    <Art hash={artist.art_hash} size={140} round={999} letter={artist.name.charAt(0).toUpperCase()} />
    <div class="heading">
      <h1>{artist.name}</h1>
      <AddToPlaylist kind="artist" refId={artist.id} label={artist.name} />
    </div>
  </header>

  {#if artist.bio}
    <section class="bio">
      <p class:clamped={!bioOpen}>{artist.bio}</p>
      <div class="bio-links">
        <button class="link" onclick={() => (bioOpen = !bioOpen)}>{bioOpen ? 'Less' : 'More'}</button>
        {#if artist.bio_url}
          <a href={artist.bio_url} target="_blank" rel="noopener">Wikipedia</a>
        {/if}
      </div>
    </section>
  {/if}

  {#if artist.top_tracks.length}
    <h2>{ranked ? 'Top songs' : 'Songs'}</h2>
    {#each artist.top_tracks as track (track.id)}
      <TrackRow {track} note={track.requests ? plural(track.requests, 'request') : null} />
    {/each}
  {/if}

  {#if artist.albums.length}
    <h2>Albums</h2>
    <div class="albums">
      {#each artist.albums as album (album.id)}
        <a class="album" href="#/album/{album.id}">
          <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
          <span class="ellipsis">{album.title}</span>
          {#if album.year}<span class="muted small">{album.year}</span>{/if}
        </a>
      {/each}
    </div>
  {/if}

  {#if moreSongs.length}
    <h2>More songs</h2>
    {#each moreSongs as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 18px;
    margin: 4px 0 12px;
  }

  .heading {
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-size: 1.75rem;
    overflow-wrap: anywhere;
  }

  .bio p {
    margin: 0;
    line-height: 1.5;
  }

  .bio p.clamped {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .bio-links {
    display: flex;
    gap: 16px;
    align-items: center;
    margin-top: 4px;
    font-size: 0.875rem;
  }

  .link {
    min-height: 32px;
    padding: 0;
    background: transparent;
    color: var(--accent);
  }

  .albums {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 16px 14px;
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
</style>
