<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import AddToPlaylist from '../lib/AddToPlaylist.svelte'
  import GenreChips from '../lib/GenreChips.svelte'
  import QueueAll from '../lib/QueueAll.svelte'
  import SortChoice from '../lib/SortChoice.svelte'
  import TagEditor from '../lib/TagEditor.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import { byName } from '../lib/format'
  import { isAdmin } from '../lib/state.svelte'
  import type { AlbumDetail, Track } from '../lib/types'

  let { id }: { id: number } = $props()
  let album = $state<AlbumDetail | null>(null)
  let error = $state('')
  let editing = $state(false)

  $effect(() => {
    album = null
    api
      .album(id)
      .then((a) => (album = a))
      .catch((e) => (error = e.message))
  })

  /** A new title or album artist makes it a different album: follow it. */
  async function edited(trackIds: number[]) {
    editing = false
    try {
      const moved = (await api.track(trackIds[0])).album_id
      if (moved && moved !== id) location.replace(`#/album/${moved}`)
      else album = await api.album(id)
    } catch {
      // The page shows the old details until the next visit.
    }
  }

  let trackSort = $state<'number' | 'name'>('number')
  const tracks = $derived(
    album ? (trackSort === 'name' ? [...album.tracks].sort(byName<Track>((t) => t.title)) : album.tracks) : [],
  )
  const trackOrders: { value: 'number' | 'name'; label: string }[] = [
    { value: 'number', label: 'Track order' },
    { value: 'name', label: 'A to Z' },
  ]
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

{#if error}
  <p class="error">{error}</p>
{:else if album}
  <header>
    <Art hash={album.art_hash} size={160} round={12} />
    <div>
      <h1>{album.title}</h1>
      <a href="#/artist/{album.artist_id}">{album.artist}</a>
      {#if album.year}<span class="muted"> · {album.year}</span>{/if}
      {#if album.genres}
        <div class="genres"><GenreChips genres={album.genres.split(',').map((g) => g.trim())} /></div>
      {/if}
      <div class="admin-actions">
        <QueueAll kind="album" refId={album.id} />
        <AddToPlaylist kind="album" refId={album.id} label={album.title} />
        {#if isAdmin()}
          <button onclick={() => (editing = true)}><Icon name="edit" size={18} /> Edit album</button>
        {/if}
      </div>
    </div>
  </header>
  {#if album.tracks.length > 1}
    <div class="sort">
      <SortChoice options={trackOrders} bind:value={trackSort} storageKey="djinn-album-track-sort" label="Sort songs" />
    </div>
  {/if}
  {#each tracks as track (track.id)}
    <TrackRow {track} showArt={false} showAlbum={false} number={track.track_no} />
  {/each}
  {#if editing}
    {@const ids = album.tracks.map((t) => t.id)}
    <TagEditor
      title="Edit album"
      trackIds={ids}
      fields={['album', 'album_artist', 'year', 'genre']}
      initial={{
        album: album.title,
        album_artist: album.artist,
        year: album.year?.toString() ?? '',
        genre: album.genres ?? '',
      }}
      onclose={() => (editing = false)}
      onsaved={() => edited(ids)}
    />
  {/if}
{/if}

<style>
  .genres {
    margin: 6px 0 2px;
  }

  .admin-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  header {
    display: flex;
    align-items: flex-end;
    gap: 16px;
    margin-bottom: 12px;
  }

  h1 {
    margin: 0 0 4px;
    font-size: 1.375rem;
  }

  header a {
    color: var(--text);
  }

  .sort {
    display: flex;
    justify-content: flex-end;
    margin-bottom: 4px;
  }
</style>
