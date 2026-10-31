<script lang="ts">
  import AddToPlaylist from '../lib/AddToPlaylist.svelte'
  import GenreChips from '../lib/GenreChips.svelte'
  import QueueAll from '../lib/QueueAll.svelte'
  import RadioButton from '../lib/RadioButton.svelte'
  import { api } from '../lib/api'
  import ArtistSlideshow from '../lib/ArtistSlideshow.svelte'
  import ArtistMediaManager from '../lib/ArtistMediaManager.svelte'
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import SortChoice from '../lib/SortChoice.svelte'
  import TagEditor from '../lib/TagEditor.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import { byName } from '../lib/format'
  import { isAdmin } from '../lib/state.svelte'
  import type { Album, ArtistDetail, ArtistPicture, ArtistSummary, TagFields } from '../lib/types'

  let { id }: { id: number } = $props()
  let artist = $state<ArtistDetail | null>(null)
  let error = $state('')
  let bioOpen = $state(false)
  let editing = $state(false)
  let managing = $state(false)

  let similar = $state<ArtistSummary[]>([])
  let pictures = $state<ArtistPicture[]>([])
  let pictureShown = $state(0)

  $effect(() => {
    artist = null
    similar = []
    pictures = []
    bioOpen = false
    api
      .artist(id)
      .then((a) => (artist = a))
      .catch((e) => (error = e.message))
    api
      .similarArtists(id)
      .then((list) => (similar = list))
      .catch(() => {})
    // The first visit fetches them, so they can arrive a little later.
    const asked = id
    api
      .artistPictures(id)
      .then((list) => {
        if (asked === id) pictures = list
      })
      .catch(() => {})
  })

  /** A new name can make them a different artist: follow it. */
  async function saveArtist(change: TagFields) {
    const { id: now } = await api.editArtist(id, {
      name: change.artist as string | null | undefined,
      genre: change.genre as string | null | undefined,
    })
    editing = false
    if (now !== id) location.replace(`#/artist/${now}`)
    else artist = await api.artist(id)
  }

  /** Top songs earn the title once anyone has requested or played them. */
  const ranked = $derived(artist?.top_tracks.some((t) => t.requests || t.plays) ?? false)
  const topIds = $derived(new Set(artist?.top_tracks.map((t) => t.id)))
  const moreSongs = $derived(artist?.other_tracks.filter((t) => !topIds.has(t.id)) ?? [])
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`

  let albumSort = $state<'year' | 'name'>('year')
  /** Newest first (unknown years last), or A to Z. */
  function sorted(albums: Album[]): Album[] {
    const list = [...albums]
    if (albumSort === 'name') return list.sort(byName((a) => a.title))
    return list.sort((a, b) => (b.year ?? -Infinity) - (a.year ?? -Infinity) || byName<Album>((x) => x.title)(a, b))
  }
  const albumOrders: { value: 'year' | 'name'; label: string }[] = [
    { value: 'year', label: 'Newest' },
    { value: 'name', label: 'A to Z' },
  ]
</script>

{#if error}
  <p class="error">{error}</p>
{:else if artist}
  <div class="artist-page">
  {#if pictures.length}
    <div class="background"><ArtistSlideshow {pictures} src={(picture) => picture.remote_url ?? api.artUrl(picture.art_hash)} credit={false} bind:shown={pictureShown} /></div>
    <div class="shade"></div>
  {/if}
  <div class="hero" class:with-picture={pictures.length > 0}>
    <header>
      <Art hash={artist.art_hash} size={140} round={999} letter={artist.name.charAt(0).toUpperCase()} />
      <div class="heading">
        <h1>{artist.name}</h1>
        {#if artist.genres}
          <div class="genres"><GenreChips genres={artist.genres.split(', ')} /></div>
        {/if}
        <div class="admin-actions">
          <RadioButton seed={{ kind: 'artist', id: artist.id }} />
          <QueueAll kind="artist" refId={artist.id} />
          <AddToPlaylist kind="artist" refId={artist.id} label={artist.name} />
          {#if isAdmin()}
            <button class="pill" onclick={() => (editing = true)}><Icon name="edit" size={18} /> Edit artist</button>
            <button class="pill" onclick={() => (managing = true)}>Manage pictures</button>
          {/if}
        </div>
      </div>
    </header>
  </div>
  {#if pictures[pictureShown]}
    {@const picture = pictures[pictureShown]}
    <p class="credit small muted">
      Slideshow photo: {#if picture.credit_url}<a href={picture.credit_url} target="_blank" rel="noopener noreferrer">{picture.credit}</a>{:else}{picture.credit}{/if}{picture.source === 'commons' ? ', Wikimedia Commons' : ''}
    </p>
  {/if}
  {#if artist.photo_credit}
    <p class="credit small muted">
      Photo: {#if artist.photo_credit_url}<a href={artist.photo_credit_url} target="_blank" rel="noopener"
          >{artist.photo_credit}</a
        >{:else}{artist.photo_credit}{/if}, Wikimedia Commons
    </p>
  {/if}

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
    <div class="section-head">
      <h2>Albums</h2>
      {#if artist.albums.length > 1}
        <SortChoice options={albumOrders} bind:value={albumSort} storageKey="djinn-artist-album-sort" label="Sort albums" />
      {/if}
    </div>
    <div class="albums">
      {#each sorted(artist.albums) as album (album.id)}
        <a class="album" href="#/album/{album.id}">
          <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
          <span class="ellipsis">{album.title}</span>
          {#if album.year}<span class="muted small">{album.year}</span>{/if}
        </a>
      {/each}
    </div>
  {/if}

  {#if artist.appears_on.length}
    <h2>Appears on</h2>
    <div class="albums">
      {#each sorted(artist.appears_on) as album (album.id)}
        <a class="album" href="#/album/{album.id}">
          <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
          <span class="ellipsis">{album.title}</span>
          <span class="muted small ellipsis">{album.artist}</span>
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
  {#if similar.length}
    <h2>Similar artists</h2>
    <div class="artists">
      {#each similar as other (other.id)}
        <a class="artist" href="#/artist/{other.id}">
          <Art hash={other.art_hash} size={96} round={999} letter={other.name.charAt(0).toUpperCase()} />
          <span class="ellipsis">{other.name}</span>
        </a>
      {/each}
    </div>
  {/if}
  {#if editing}
    <TagEditor
      title="Edit artist"
      save={saveArtist}
      applies="A new name goes on every song and album they're on; genres go on the songs they lead."
      fields={['artist', 'genre']}
      initial={{ artist: artist.name, genre: artist.genres ?? '' }}
      onclose={() => (editing = false)}
      onsaved={() => (editing = false)}
    />
  {/if}
  {#if managing}
    <ArtistMediaManager
      {id}
      name={artist.name}
      onclose={() => (managing = false)}
      onchange={() => api.artistPictures(id).then((list) => (pictures = list)).catch(() => {})}
    />
  {/if}
  </div>
{/if}

<style>
  .artist-page {
    position: relative;
  }

  .artist-page > :not(.background):not(.shade) {
    position: relative;
    z-index: 1;
  }

  .hero {
    position: relative;
  }

  .hero.with-picture {
    min-height: 0;
  }

  .background,
  .shade {
    position: fixed;
    z-index: 0;
    inset: 0;
    pointer-events: none;
  }

  .background {
    opacity: 0.32;
    filter: blur(10px) saturate(0.75);
  }

  .shade {
    background: color-mix(in srgb, var(--bg) 55%, transparent);
  }

  .hero header {
    position: relative;
    z-index: 1;
  }

  .hero.with-picture h1 {
    color: var(--text);
  }

  .admin-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

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

  .genres {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 6px;
  }

  .credit {
    margin: -4px 0 10px;
  }

  .credit a {
    color: inherit;
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

  .artists {
    display: flex;
    gap: 14px;
    overflow-x: auto;
    padding-bottom: 6px;
    scrollbar-width: none;
  }

  .artist {
    flex: none;
    width: 96px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: var(--text);
    text-decoration: none;
    font-size: 0.8125rem;
    text-align: center;
  }

  .artist span {
    max-width: 100%;
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
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
