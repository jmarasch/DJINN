<script lang="ts">
  import { t } from '../../lib/i18n.svelte'
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
      error = e instanceof Error ? e.message : t('ed.load_failed')
    }
  }

  const mine = $derived(list?.mine ?? false)
  const flagged = $derived(list?.rows.filter((row) => row.problem) ?? [])

  function reason(row: PlaylistRow) {
    if (row.problem === 'not_in_library') return t('gpl.not_in_library')
    if (row.problem === 'file_missing') return t('pl.missing_file')
    return t('pl.cant_play')
  }

  async function removeRow(row: PlaylistRow) {
    if (!list) return
    try {
      await api.removeFromPlaylist(list.id, row.entry_id)
      await load(list.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.remove_failed'), true)
    }
  }

  async function rename() {
    if (!list) return
    const name = prompt(t('gpl.name_q'), list.name)
    if (!name) return
    try {
      await api.goUpdatePlaylist(list.id, { name })
      list.name = name.trim()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gpl.rename_failed'), true)
    }
  }

  async function setShared(shared: boolean) {
    if (!list) return
    try {
      await api.goUpdatePlaylist(list.id, { shared })
      list.shared = shared
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ps.failed'), true)
    }
  }

  async function remove() {
    if (!list || !confirm(t('gpl.delete_q', { name: list.name }))) return
    try {
      await api.goDeletePlaylist(list.id)
      location.hash = '#/library'
    } catch (e) {
      toast(e instanceof Error ? e.message : t('pl.delete_failed'), true)
    }
  }

  async function saveSongs(songs: number[]) {
    if (!list) return
    try {
      await api.goSetPlaylistSongs(list.id, songs)
      await load(list.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('lf.change_failed'), true)
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
    {t('count.songs', { n: list.songs.length })}{#if !mine}{t('gpl.shared_by', { owner: list.owner })}{:else if list.shared}{t(
        'gpl.shared',
      )}{/if}{#if flagged.length}<span class="bad"><Icon name="warning" size={13} />{t('gpl.problems', { n: flagged.length })}</span>{/if}
  </p>
  <div class="buttons">
    <button class="primary" onclick={() => list && playList(list.songs)} disabled={!list.songs.length}>
      <Icon name="play" /> {t('pl.play')}
    </button>
    <button onclick={() => list && playNext(list.songs)} disabled={!list.songs.length}>{t('gpl.play_next')}</button>
    <button onclick={() => list && addToEnd(list.songs)} disabled={!list.songs.length}>{t('gpl.add_end')}</button>
    {#if mine}
      <button onclick={() => (editing = !editing)}>{editing ? t('gpl.done') : t('gpl.edit')}</button>
    {/if}
  </div>
  {#if editing && mine}
    <div class="edit card">
      <button class="small-button" onclick={rename}>{t('common.rename')}</button>
      <button class="small-button" onclick={remove}>{t('pl.delete')}</button>
    </div>
    <PlaylistSharing id={list.id} shared={list.shared} {setShared} />
    {#each list.songs as track, i (`${track.id}-${i}`)}
      <div class="edit-row">
        <span class="grow">{track.title} <span class="muted small">{track.artist ?? ''}</span></span>
        <button class="icon" aria-label={t('player.move_up')} onclick={() => move(i, -1)} disabled={i === 0}>
          <Icon name="up" />
        </button>
        <button class="icon" aria-label={t('player.move_down')} onclick={() => move(i, 1)} disabled={i === list.songs.length - 1}>
          <Icon name="down" />
        </button>
        <button class="icon" aria-label={t('pl.remove', { name: track.title })} onclick={() => drop(i)}><Icon name="close" /></button>
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
            <button class="icon" aria-label={t('pl.remove', { name: row.title ?? '' })} onclick={() => removeRow(row)}><Icon name="close" /></button>
          {/if}
        </div>
      {/if}
    {:else}
      <p class="muted">{t('gpl.none')}</p>
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
