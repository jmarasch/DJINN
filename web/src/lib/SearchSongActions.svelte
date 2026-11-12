<script lang="ts">
  // Under a search's songs: keep them as a playlist, or make that playlist
  // the party's auto mix right away.
  import { api } from './api'
  import { t } from './i18n.svelte'
  import { canStartAutoMix, hasPlaylists, toast } from './state.svelte'
  import { playlistName } from './format'
  import type { Track } from './types'

  let { name, tracks }: { name: string; tracks: Track[] } = $props()
  let busy = $state(false)

  async function save(andPlay: boolean) {
    busy = true
    try {
      const playlist = await api.playlistFromSongs(
        playlistName(name),
        tracks.map((track) => track.id),
      )
      if (andPlay) {
        await api.playPlaylist(playlist.id)
        toast(t('search.auto_mix_started', { name: playlist.name }))
      } else {
        toast(t('search.made_playlist', { name: playlist.name }))
        location.hash = `/playlist/${playlist.id}`
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('search.playlist_failed'), true)
    } finally {
      busy = false
    }
  }
</script>

{#if hasPlaylists() && tracks.length}
  <div class="actions">
    <button class="small-button" disabled={busy} onclick={() => save(false)}>{t('search.save_playlist')}</button>
    {#if canStartAutoMix()}
      <button class="small-button" disabled={busy} onclick={() => save(true)}>{t('search.play_as_auto_mix')}</button>
    {/if}
  </div>
{/if}

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0 0 8px;
  }
</style>
