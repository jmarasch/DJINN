<script lang="ts">
  // One of your playlists (or one someone shared).
  import { api } from '../../lib/api'
  import Icon from '../../lib/Icon.svelte'
  import { app, toast } from '../../lib/state.svelte'
  import type { GoPlaylistDetail } from '../../lib/types'
  import { addToQueue, playList } from '../player.svelte'
  import SongRow from '../SongRow.svelte'

  let { id }: { id: number } = $props()
  let list = $state<GoPlaylistDetail | null>(null)
  let error = $state('')
  let editing = $state(false)

  $effect(() => {
    load(id)
  })

  async function load(which: number) {
    try {
      list = await api.goPlaylist(which)
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not load'
    }
  }

  const mine = $derived(list?.user_id === app.me?.id)

  async function rename() {
    if (!list) return
    const name = prompt('Playlist name', list.name)
    if (!name) return
    try {
      await api.goUpdatePlaylist(list.id, { name })
      list.name = name.trim()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not rename it', true)
    }
  }

  async function setShared(shared: boolean) {
    if (!list) return
    try {
      await api.goUpdatePlaylist(list.id, { shared })
      list.shared = shared
      toast(shared ? 'Other GO listeners can see and play it now' : 'Only you can see it now')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change that', true)
    }
  }

  async function remove() {
    if (!list || !confirm(`Delete ${list.name}?`)) return
    try {
      await api.goDeletePlaylist(list.id)
      location.hash = '#/library'
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not delete it', true)
    }
  }

  async function saveSongs(songs: number[]) {
    if (!list) return
    try {
      await api.goSetPlaylistSongs(list.id, songs)
      await load(list.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change it', true)
    }
  }

  const ids = () => list?.songs.map((t) => t.id) ?? []

  function move(i: number, by: number) {
    const songs = ids()
    const j = i + by
    if (j < 0 || j >= songs.length) return
    ;[songs[i], songs[j]] = [songs[j], songs[i]]
    saveSongs(songs)
  }

  function drop(i: number) {
    const songs = ids()
    songs.splice(i, 1)
    saveSongs(songs)
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if list}
  <h1>{list.name}</h1>
  <p class="muted small">
    {list.songs.length}
    {list.songs.length === 1 ? 'song' : 'songs'}{#if !mine}, shared by {list.owner}{:else if list.shared}, shared{/if}
  </p>
  <div class="buttons">
    <button class="primary" onclick={() => list && playList(list.songs)} disabled={!list.songs.length}>
      <Icon name="play" /> Play
    </button>
    <button onclick={() => list && addToQueue(list.songs)} disabled={!list.songs.length}>Add to queue</button>
    {#if mine}
      <button onclick={() => (editing = !editing)}>{editing ? 'Done' : 'Edit'}</button>
    {/if}
  </div>
  {#if editing && mine}
    <div class="edit card">
      <button class="small-button" onclick={rename}>Rename</button>
      {#if app.me?.permissions?.share_playlists}
        <label class="switch">
          <input type="checkbox" checked={list.shared} onchange={(e) => setShared(e.currentTarget.checked)} />
          Share with other GO listeners
        </label>
      {/if}
      <button class="small-button" onclick={remove}>Delete playlist</button>
    </div>
    {#each list.songs as track, i (`${track.id}-${i}`)}
      <div class="edit-row">
        <span class="grow">{track.title} <span class="muted small">{track.artist ?? ''}</span></span>
        <button class="icon" aria-label="Move up" onclick={() => move(i, -1)} disabled={i === 0}>
          <Icon name="up" />
        </button>
        <button class="icon" aria-label="Move down" onclick={() => move(i, 1)} disabled={i === list.songs.length - 1}>
          <Icon name="down" />
        </button>
        <button class="icon" aria-label="Remove {track.title}" onclick={() => drop(i)}><Icon name="close" /></button>
      </div>
    {/each}
  {:else}
    {#each list.songs as track, i (`${track.id}-${i}`)}
      <SongRow {track} onplay={() => list && playList(list.songs, i)} />
    {:else}
      <p class="muted">No songs yet. Add some from a song's menu.</p>
    {/each}
  {/if}
{/if}

<style>
  h1 {
    margin: 0;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 8px 0 12px;
  }

  .edit {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
  }

  .edit-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 0;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .icon {
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }
</style>
