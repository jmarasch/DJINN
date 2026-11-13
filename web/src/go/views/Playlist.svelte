<script lang="ts">
  import PageTitle from '../../lib/PageTitle.svelte'
  // One of your playlists (or one someone shared).
  import { api } from '../../lib/api'
  import Icon from '../../lib/Icon.svelte'
  import { app, toast } from '../../lib/state.svelte'
  import PlaylistSharing from '../../lib/PlaylistSharing.svelte'
  import type { GoPlaylistDetail, PlaylistRow } from '../../lib/types'
  import { addToEnd, playList, playNext, tapSong } from '../player.svelte'
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

  const mine = $derived(list?.mine ?? false)
  const flagged = $derived(list?.rows.filter((row) => row.problem) ?? [])

  function reason(row: PlaylistRow) {
    if (row.problem === 'not_in_library') return 'Not in the library'
    if (row.problem === 'file_missing') return 'File is missing'
    return "File won't play"
  }

  async function removeRow(row: PlaylistRow) {
    if (!list) return
    try {
      await api.removeFromPlaylist(list.id, row.entry_id)
      await load(list.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove it', true)
    }
  }

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
  <PageTitle title={list.name} />
  <p class="muted small">
    {list.songs.length}
    {list.songs.length === 1 ? 'song' : 'songs'}{#if !mine}, shared by {list.owner}{:else if list.shared}, shared{/if}{#if flagged.length},
      <span class="bad"><Icon name="warning" size={13} /> {flagged.length} can't play and will be skipped</span>{/if}
  </p>
  <div class="buttons">
    <button class="primary" onclick={() => list && playList(list.songs)} disabled={!list.songs.length}>
      <Icon name="play" /> Play
    </button>
    <button onclick={() => list && playNext(list.songs)} disabled={!list.songs.length}>Play next</button>
    <button onclick={() => list && addToEnd(list.songs)} disabled={!list.songs.length}>Add to end of queue</button>
    {#if mine}
      <button onclick={() => (editing = !editing)}>{editing ? 'Done' : 'Edit'}</button>
    {/if}
  </div>
  {#if editing && mine}
    <div class="edit card">
      <button class="small-button" onclick={rename}>Rename</button>
      <button class="small-button" onclick={remove}>Delete playlist</button>
    </div>
    <PlaylistSharing id={list.id} shared={list.shared} {setShared} />
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
    {#each list.rows as row, i (`${row.entry_id}-${row.track_id}-${i}`)}
      {@const at = row.song_index}
      {@const song = at === null ? undefined : list.songs[at]}
      {#if song}
        <SongRow track={song} onplay={() => tapSong(song, () => list && playList(list.songs, at ?? 0))} />
      {:else}
        <div class="edit-row flagged">
          <span class="grow">
            <span class="block">{row.title} <span class="muted small">{row.artist ?? ''}</span></span>
            <span class="bad small" title={row.error ?? undefined}><Icon name="warning" size={13} /> {reason(row)}</span>
          </span>
          {#if mine && row.removable}
            <button class="icon" aria-label="Remove {row.title}" onclick={() => removeRow(row)}><Icon name="close" /></button>
          {/if}
        </div>
      {/if}
    {:else}
      <p class="muted">No songs yet. Add some from a song's menu.</p>
    {/each}
  {/if}
{/if}

<style>

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

  .bad {
    color: var(--bad);
  }

  .block {
    display: block;
  }

  .flagged {
    min-height: 56px;
    color: var(--muted);
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
