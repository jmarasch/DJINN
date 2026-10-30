<script lang="ts">
  // Everything you can do with one song in DJINN GO.
  import { api } from '../lib/api'
  import Sheet from '../lib/Sheet.svelte'
  import { app, toast } from '../lib/state.svelte'
  import { addToQueue, likes, playNext, startRadio, toggleLike } from './player.svelte'
  import { ui } from './ui.svelte'

  const track = $derived(ui.menu)
  const close = () => (ui.menu = null)

  async function sendToParty() {
    if (!track) return
    const song = track
    close()
    try {
      const { outcome } = await api.goSendToParty(song.id)
      toast(outcome === 'backed' ? `Someone asked for "${song.title}" already; you backed it` : `Sent "${song.title}" to the party`)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not send it to the party', true)
    }
  }

  async function download() {
    if (!track) return
    const song = track
    close()
    try {
      const links = await api.playLinks(song.id)
      if (!links.download_url) return toast("Your account can't download songs", true)
      const a = document.createElement('a')
      a.href = links.download_url
      a.download = ''
      a.click()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not download it', true)
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
      <button onclick={() => act(() => playNext(song))}>Play next</button>
      <button onclick={() => act(() => addToQueue([song]))}>Add to queue</button>
      <button onclick={() => act(() => startRadio(song))}>Start radio from this song</button>
      <button onclick={() => act(() => toggleLike('track', song.id))}>
        {likes.track.has(song.id) ? 'Unlike' : 'Like'}
      </button>
      <button onclick={() => act(() => (ui.adding = [song]))}>Add to a playlist</button>
      {#if song.album_id}<a class="button" href="#/album/{song.album_id}" onclick={close}>Go to album</a>{/if}
      {#if song.artist_id}<a class="button" href="#/artist/{song.artist_id}" onclick={close}>Go to artist</a>{/if}
      <button onclick={sendToParty}>Send to the party</button>
      {#if app.me?.permissions?.downloads}<button onclick={download}>Download</button>{/if}
    </div>
  </Sheet>
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
