<script lang="ts">
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import Icon from '../../lib/Icon.svelte'
  import type { AlbumDetail } from '../../lib/types'
  import { addToQueue, likes, playList, toggleLike } from '../player.svelte'
  import SongList from '../SongList.svelte'
  import { ui } from '../ui.svelte'

  let { id }: { id: number } = $props()
  let album = $state<AlbumDetail | null>(null)
  let error = $state('')

  $effect(() => {
    album = null
    api
      .album(id)
      .then((a) => (album = a))
      .catch((e) => (error = e.message))
  })
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if album}
  <header>
    <Art hash={album.art_hash} size={140} />
    <div>
      <h1>{album.title}</h1>
      <a href="#/artist/{album.artist_id}">{album.artist}</a>
      {#if album.year}<span class="muted"> · {album.year}</span>{/if}
    </div>
  </header>
  <div class="buttons">
    <button class="primary" onclick={() => album && playList(album.tracks)}><Icon name="play" /> Play</button>
    <button onclick={() => album && addToQueue(album.tracks)}>Add to queue</button>
    <button onclick={() => album && (ui.adding = album.tracks)}>Add to playlist</button>
    <button
      class="icon"
      class:liked={likes.album.has(album.id)}
      aria-pressed={likes.album.has(album.id)}
      aria-label="Like this album"
      onclick={() => album && toggleLike('album', album.id)}
    >
      <Icon name={likes.album.has(album.id) ? 'heart' : 'heart-outline'} />
    </button>
  </div>
  <SongList tracks={album.tracks} numbered />
{/if}

<style>
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 16px;
    margin-bottom: 12px;
  }

  h1 {
    margin: 0 0 4px;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }

  .icon.liked {
    color: var(--accent);
  }
</style>
