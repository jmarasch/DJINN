<script lang="ts">
  // Everything on the server, in the order you choose.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import InfiniteList from '../../lib/InfiniteList.svelte'
  import type { AlbumFilter, AlbumSort, ArtistSort, Track, TrackSort } from '../../lib/types'
  import { playRadio, tapSong } from '../player.svelte'
  import SongRow from '../SongRow.svelte'

  let { section }: { section: 'artists' | 'albums' | 'songs' } = $props()

  // These last for the session, like the party's own lists.
  let artistSort = $state((sessionStorage.getItem('djinn-go-artist-sort') as ArtistSort) || 'name')
  let albumSort = $state((sessionStorage.getItem('djinn-go-album-sort') as AlbumSort) || 'name')
  let albumFilter = $state<AlbumFilter>('all')
  let songSort = $state((sessionStorage.getItem('djinn-go-song-sort') as TrackSort) || 'title')
  $effect(() => sessionStorage.setItem('djinn-go-artist-sort', artistSort))
  $effect(() => sessionStorage.setItem('djinn-go-album-sort', albumSort))
  $effect(() => sessionStorage.setItem('djinn-go-song-sort', songSort))

  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`
  const play = (track: Track) => tapSong(track, () => playRadio(track))
</script>

{#if section === 'artists'}
  <label class="sort">
    <span class="muted small">Sort by</span>
    <select bind:value={artistSort}>
      <option value="name">Name</option>
      <option value="added">Recently added</option>
    </select>
  </label>
    <InfiniteList
      cacheKey="go-artists-{artistSort}"
      load={(offset) => api.artists(artistSort, offset)}
      key={(a) => a.id}
      count={(n) => plural(n, 'artist', 'artists')}
    >
      {#snippet item(artist)}
        <a class="row" href="#/artist/{artist.id}">
          <Art hash={artist.art_hash} size={44} round={999} letter={artist.name[0]} />
          <span class="name">{artist.name}</span>
          <span class="muted small">{plural(artist.track_count, 'song', 'songs')}</span>
        </a>
      {/snippet}
    </InfiniteList>
{:else if section === 'albums'}
  <div class="choices">
    <label class="sort">
      <span class="muted small">Show</span>
      <select bind:value={albumFilter}>
        <option value="all">All albums</option>
        <option value="various">Various artists</option>
        <option value="soundtracks">Soundtracks</option>
      </select>
    </label>
    <label class="sort">
      <span class="muted small">Sort by</span>
      <select bind:value={albumSort}>
        <option value="name">Name</option>
        <option value="added">Recently added</option>
        <option value="year">Year (newest first)</option>
        <option value="oldest">Year (oldest first)</option>
      </select>
    </label>
  </div>
    <InfiniteList
      cacheKey="go-albums-{albumSort}-{albumFilter}"
      load={(offset) => api.albums(albumSort, albumFilter, offset)}
      key={(a) => a.id}
      count={(n) => plural(n, 'album', 'albums')}
    >
      {#snippet item(album)}
        <a class="row" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={44} letter={album.title[0]} />
          <span class="name">
            {album.title}
            <span class="muted small block">{album.artist}{album.year ? ` · ${album.year}` : ''}</span>
          </span>
        </a>
      {/snippet}
    </InfiniteList>
{:else}
  <label class="sort">
    <span class="muted small">Sort by</span>
    <select bind:value={songSort}>
      <option value="title">Title</option>
      <option value="artist">Artist</option>
      <option value="added">Recently added</option>
      <option value="popular">Most popular</option>
      <option value="year">Year (newest first)</option>
      <option value="oldest">Year (oldest first)</option>
    </select>
  </label>
    <InfiniteList
      cacheKey="go-songs-{songSort}"
      load={(offset) => api.tracks(songSort, offset)}
      key={(t) => t.id}
      count={(n) => plural(n, 'song', 'songs')}
    >
      {#snippet item(track)}
        <SongRow {track} onplay={() => play(track)} />
      {/snippet}
    </InfiniteList>
{/if}

<style>
  .sort {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    color: var(--text);
    text-decoration: none;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .block {
    display: block;
  }
</style>
