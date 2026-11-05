<script lang="ts">
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import ScrollRow from '../../lib/ScrollRow.svelte'
  import Icon from '../../lib/Icon.svelte'
  import type { ArtistDetail, ArtistSummary } from '../../lib/types'
  import { likes, playList, startRadio, toggleLike } from '../player.svelte'
  import SongList from '../SongList.svelte'

  let { id }: { id: number } = $props()
  let artist = $state<ArtistDetail | null>(null)
  let similar = $state<ArtistSummary[]>([])
  let error = $state('')

  $effect(() => {
    artist = null
    similar = []
    api
      .artist(id)
      .then((a) => (artist = a))
      .catch((e) => (error = e.message))
    api
      .similarArtists(id)
      .then((list) => (similar = list))
      .catch(() => {})
  })

  /** Every song by the artist: top ones, then the rest. */
  const songs = $derived(artist ? [...artist.top_tracks, ...artist.other_tracks] : [])
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if artist}
  <header>
    <Art hash={artist.art_hash} size={120} round={999} letter={artist.name[0]} />
    <h1>{artist.name}</h1>
  </header>
  <div class="buttons">
    <button class="primary" onclick={() => playList(songs)} disabled={!songs.length}><Icon name="play" /> Play</button>
    {#if songs[0]}<button onclick={() => startRadio(songs[0])}><Icon name="radio" /> Radio</button>{/if}
    <button
      class="icon"
      class:liked={likes.artist.has(artist.id)}
      aria-pressed={likes.artist.has(artist.id)}
      aria-label="Like this artist"
      onclick={() => artist && toggleLike('artist', artist.id)}
    >
      <Icon name={likes.artist.has(artist.id) ? 'heart' : 'heart-outline'} />
    </button>
  </div>
  {#if artist.top_tracks.length}
    <h2>Songs</h2>
    <SongList tracks={artist.top_tracks} />
  {/if}
  {#if artist.albums.length}
    <h2>Albums</h2>
    <ScrollRow gap={12}>
      {#each artist.albums as album (album.id)}
        <a class="tile" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={120} />
          <span class="small">{album.title}</span>
          {#if album.year}<span class="muted small">{album.year}</span>{/if}
        </a>
      {/each}
    </ScrollRow>
  {/if}
  {#if artist.appears_on.length}
    <h2>Appears on</h2>
    <ScrollRow gap={12}>
      {#each artist.appears_on as album (album.id)}
        <a class="tile" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={120} />
          <span class="small">{album.title}</span>
        </a>
      {/each}
    </ScrollRow>
  {/if}
  {#if similar.length}
    <h2>Similar artists</h2>
    <ScrollRow>
      {#each similar as other (other.id)}
        <a class="other" href="#/artist/{other.id}">
          <Art hash={other.art_hash} size={96} round={999} letter={other.name.charAt(0).toUpperCase()} />
          <span class="small">{other.name}</span>
        </a>
      {/each}
    </ScrollRow>
  {/if}
  {#if artist.bio}<p class="bio muted">{artist.bio}</p>{/if}
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 12px;
  }

  h1 {
    margin: 0;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .icon.liked {
    color: var(--accent);
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: inherit;
    text-decoration: none;
  }

  .tile span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .other {
    flex: none;
    width: 96px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: inherit;
    text-align: center;
    text-decoration: none;
  }

  .other span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bio {
    margin-top: 20px;
    white-space: pre-line;
  }
</style>
