<script lang="ts">
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import ScrollRow from '../../lib/ScrollRow.svelte'
  import Icon from '../../lib/Icon.svelte'
  import type { ArtistDetail, ArtistSummary, Track } from '../../lib/types'
  import { toast } from '../../lib/state.svelte'
  import { likes, playList, startRadio, toggleLike } from '../player.svelte'
  import SongList from '../SongList.svelte'

  let { id }: { id: number } = $props()
  let artist = $state<ArtistDetail | null>(null)
  let similar = $state<ArtistSummary[]>([])
  let error = $state('')

  // Every song by the artist: the top ones first, the rest on Show more.
  let allSongs = $state<Track[] | null>(null)
  let songTotal = $state(0)
  let loadingSongs = $state(false)

  $effect(() => {
    allSongs = null
    songTotal = 0
    api
      .artistSongs(id, 0, 1)
      .then((page) => (songTotal = page.total))
      .catch(() => {})
  })

  async function showMore() {
    loadingSongs = true
    try {
      const page = await api.artistSongs(id, allSongs?.length ?? 0, 100)
      allSongs = [...(allSongs ?? []), ...page.tracks]
      songTotal = page.total
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load more songs', true)
    } finally {
      loadingSongs = false
    }
  }

  /** Plays every song by the artist, shown or not. */
  async function playAll() {
    try {
      const page = await api.artistSongs(id, 0, 5000)
      if (page.tracks.length) playList(page.tracks)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not play them', true)
    }
  }

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

  const songs = $derived(artist ? (allSongs ?? artist.top_tracks) : [])
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if artist}
  <header>
    <Art hash={artist.art_hash} size={120} round={999} letter={artist.name[0]} />
    <h1>{artist.name}</h1>
  </header>
  <div class="buttons">
    <button class="primary" onclick={playAll} disabled={!songTotal}><Icon name="play" /> Play all</button>
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
  {#if songs.length}
    <h2>Songs{#if songTotal}<span class="muted small">{` (${songTotal.toLocaleString()})`}</span>{/if}</h2>
    <SongList tracks={songs} />
    {#if songs.length < songTotal}
      <button class="more" disabled={loadingSongs} onclick={showMore}>
        {allSongs ? 'Show more' : `Show all ${songTotal.toLocaleString()} songs`}
      </button>
    {/if}
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
  .more {
    width: 100%;
    margin-top: 8px;
  }

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
