<script lang="ts">
  // Everything you can do with one song in DJINN GO.
  import { api } from '../lib/api'
  import { t } from '../lib/i18n.svelte'
  import ReportSong from '../lib/ReportSong.svelte'
  import Sheet from '../lib/Sheet.svelte'
  import { app, toast } from '../lib/state.svelte'
  import { addToEnd, likes, playNext, startRadio, toggleLike } from './player.svelte'
  import { ui } from './ui.svelte'

  const track = $derived(ui.menu)
  const close = () => (ui.menu = null)
  let reporting = $state<{ id: number; title: string } | null>(null)

  async function sendToParty() {
    if (!track) return
    const song = track
    close()
    try {
      const { outcome } = await api.goSendToParty(song.id)
      toast(outcome === 'backed' ? t('gm.backed', { title: song.title }) : t('gm.sent', { title: song.title }))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gm.send_failed'), true)
    }
  }

  async function download() {
    if (!track) return
    const song = track
    close()
    try {
      const links = await api.playLinks(song.id)
      if (!links.download_url) return toast(t('gm.no_download'), true)
      const a = document.createElement('a')
      a.href = links.download_url
      a.download = ''
      a.click()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gm.download_failed'), true)
    }
  }

  function act(action: () => void) {
    action()
    close()
  }
</script>

{#if track}
  {@const song = track}
  <Sheet title={song.title} onclose={close}>
    <div class="actions">
      <button onclick={() => act(() => playNext([song]))}>{t('menu.play_next')}</button>
      <button onclick={() => act(() => addToEnd([song]))}>{t('menu.add_to_end')}</button>
      <button onclick={() => act(() => startRadio(song))}>{t('menu.start_radio_song')}</button>
      <button onclick={() => act(() => toggleLike('track', song.id))}>
        {likes.track.has(song.id) ? t('player.unlike') : t('player.like')}
      </button>
      <button onclick={() => act(() => (ui.adding = [song]))}>{t('menu.add_to_playlist')}</button>
      {#if song.album_id}<a class="button" href="#/album/{song.album_id}" onclick={close}>{t('menu.go_to_album')}</a>{/if}
      {#if song.artist_id}<a class="button" href="#/artist/{song.artist_id}" onclick={close}>{t('menu.go_to_artist')}</a>{/if}
      <button onclick={sendToParty}>{t('menu.send_to_party')}</button>
      {#if app.me?.permissions?.downloads}<button onclick={download}>{t('menu.download')}</button>{/if}
      <button onclick={() => act(() => (reporting = { id: song.id, title: song.title }))}>{t('menu.report')}</button>
    </div>
  </Sheet>
{/if}
{#if reporting}
  <ReportSong song={reporting} onclose={() => (reporting = null)} />
{/if}

<style>
  .actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .actions > * {
    justify-content: flex-start;
    width: 100%;
  }
</style>
