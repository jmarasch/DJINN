<script lang="ts">
  // One genre: its songs, artists and related genres.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import Icon from '../../lib/Icon.svelte'
  import InfiniteList from '../../lib/InfiniteList.svelte'
  import ScrollRow from '../../lib/ScrollRow.svelte'
  import type { ArtistSummary, GenrePage, Track } from '../../lib/types'
  import { playList, setShuffle, tapSong } from '../player.svelte'
  import SongRow from '../SongRow.svelte'
  import LikeButton from '../LikeButton.svelte'

  let { name }: { name: string } = $props()
  let page = $state<GenrePage | null>(null)
  let error = $state('')
  let artists = $state<ArtistSummary[]>([])
  let fetchingArtists = false
  /** The songs loaded so far, for playing on from one of them. */
  let loaded: Track[] = []

  $effect(() => {
    page = null
    error = ''
    artists = []
    loaded = []
    api
      .genre(name)
      .then((p) => {
        page = p
        artists = p.artists
        loaded = p.tracks
      })
      .catch((e) => (error = e.message))
  })

  async function moreArtists() {
    if (fetchingArtists || !page || artists.length >= page.artist_total) return
    fetchingArtists = true
    try {
      const next = await api.genreArtists(name, artists.length)
      if (next.items.length) artists = [...artists, ...next.items]
    } finally {
      fetchingArtists = false
    }
  }

  async function moreSongs(offset: number) {
    const next = await api.genreSongs(name, offset)
    const known = new Set(loaded.map((t) => t.id))
    loaded = [...loaded, ...next.items.filter((t) => !known.has(t.id))]
    return next
  }

  function playFrom(track: Track) {
    const at = loaded.findIndex((t) => t.id === track.id)
    tapSong(track, () => playList(loaded, Math.max(0, at)))
  }

  const title = $derived(name.charAt(0).toUpperCase() + name.slice(1))

  function shuffleAll() {
    if (!page?.tracks.length) return
    setShuffle(true)
    playList(page.tracks, Math.floor(Math.random() * page.tracks.length))
  }
</script>

<h1>{title}</h1>
{#if error}
  <p class="error">{error}</p>
{:else if page}
  <p class="small muted">
    {page.total}
    {page.total === 1 ? 'song' : 'songs'}, shuffled with the most played nearer the top.
  </p>
  <div class="buttons">
    <button class="primary" onclick={() => page && playList(page.tracks)} disabled={!page.tracks.length}>
      <Icon name="play" /> Play
    </button>
    <button onclick={shuffleAll} disabled={!page.tracks.length}><Icon name="shuffle" /> Shuffle</button>
  </div>
  {#if page.related.length}
    <h2>Related genres</h2>
    <div class="chips">
      {#each page.related as genre (genre.name)}
        <a class="chip" href="#/genre/{encodeURIComponent(genre.name)}">{genre.name}</a>
      {/each}
    </div>
  {/if}
  {#if artists.length}
    <h2>Artists <span class="muted small">{page.artist_total}</span></h2>
    <ScrollRow gap={12} nearEnd={moreArtists}>
      {#each artists as artist (artist.id)}
        <div class="tile-wrap">
          <a class="artist" href="#/artist/{artist.id}">
            <Art hash={artist.art_hash} size={88} round={999} letter={artist.name[0]} />
            <span class="small">{artist.name}</span>
          </a>
          <div class="corner"><LikeButton kind="artist" id={artist.id} name={artist.name} size={18} /></div>
        </div>
      {/each}
    </ScrollRow>
  {/if}
  <h2>Songs</h2>
  <InfiniteList cacheKey="go-genre-{name}" load={moreSongs} key={(t) => t.id}>
    {#snippet item(track)}
      <SongRow {track} art onplay={() => playFrom(track)} />
    {/snippet}
  </InfiniteList>
{:else}
  <p class="muted">Loading...</p>
{/if}

<style>
  h1 {
    margin: 0;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .artist {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 4px;
    width: 88px;
    align-items: center;
    color: inherit;
    text-decoration: none;
    text-align: center;
  }

  .tile-wrap {
    position: relative;
    flex: none;
  }

  /* The heart sits on the picture's corner. */
  .corner {
    position: absolute;
    top: 0;
    right: 0;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.45);
  }

  .corner :global(button:not(.liked)) {
    color: #fff;
  }
</style>
