<script lang="ts">
  // "Not in the library?" under search results: ask the admin for a song.
  // Shown to admins, and to everyone when the admin allows it.
  import { api } from './api'
  import Sheet from './Sheet.svelte'
  import { toast } from './state.svelte'

  let { query }: { query: string } = $props()

  let allowed = $state(false)
  let asking = $state(false)
  let artist = $state('')
  let title = $state('')
  let sending = $state(false)

  $effect(() => {
    api
      .wishesEnabled()
      .then((on) => (allowed = on))
      .catch(() => {})
  })

  function open() {
    title = query.trim()
    artist = ''
    asking = true
  }

  async function send(e: SubmitEvent) {
    e.preventDefault()
    sending = true
    try {
      await api.addWish(artist.trim(), title.trim())
      toast("Asked. The admin will see it.")
      asking = false
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not send it', true)
    } finally {
      sending = false
    }
  }
</script>

{#if allowed && query.trim()}
  <p class="ask small muted">
    Not in the library? <button class="link" onclick={open}>Ask for it</button>
  </p>
{/if}
{#if asking}
  <Sheet title="Ask for a song" onclose={() => (asking = false)}>
    <form onsubmit={send}>
      <label>
        Artist
        <input bind:value={artist} maxlength="200" required />
      </label>
      <label>
        Song
        <input bind:value={title} maxlength="200" required />
      </label>
      <button class="primary" disabled={sending || !artist.trim() || !title.trim()}>Ask</button>
    </form>
  </Sheet>
{/if}

<style>
  .ask {
    margin: 16px 0;
    text-align: center;
  }

  .link {
    display: inline;
    min-height: 0;
    padding: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
</style>
