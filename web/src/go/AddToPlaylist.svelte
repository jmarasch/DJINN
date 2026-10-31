<script lang="ts">
  // Picks one of your playlists (or makes a new one) for songs.
  import { api } from '../lib/api'
  import Sheet from '../lib/Sheet.svelte'
  import { app, toast } from '../lib/state.svelte'
  import type { GoPlaylist } from '../lib/types'
  import { ui } from './ui.svelte'

  let lists = $state<GoPlaylist[]>([])
  let name = $state('')

  $effect(() => {
    if (ui.adding) {
      api
        .goPlaylists()
        .then((all) => (lists = all.filter((l) => l.user_id === app.me?.id)))
        .catch(() => (lists = []))
    }
  })

  const count = () => ui.adding?.length ?? 0
  const songs = () => (ui.adding ?? []).map((t) => t.id)
  const what = () => (count() === 1 ? `"${ui.adding![0].title}"` : `${count()} songs`)

  async function add(list: GoPlaylist) {
    const label = what()
    try {
      await api.goAddToPlaylist(list.id, songs())
      toast(`Added ${label} to ${list.name}`)
      ui.adding = null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add them', true)
    }
  }

  async function create(e: SubmitEvent) {
    e.preventDefault()
    const label = what()
    try {
      const list = await api.goCreatePlaylist(name, songs())
      toast(`Made ${list.name} with ${label}`)
      name = ''
      ui.adding = null
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not make it', true)
    }
  }
</script>

{#if ui.adding}
  <Sheet title="Add to a playlist" onclose={() => (ui.adding = null)}>
    <form class="new" onsubmit={create}>
      <input bind:value={name} placeholder="New playlist" maxlength="100" required aria-label="New playlist name" />
      <button class="primary">Make</button>
    </form>
    {#each lists as list (list.id)}
      <button class="list" onclick={() => add(list)}>
        {list.name} <span class="muted small">{list.tracks} {list.tracks === 1 ? 'song' : 'songs'}</span>
      </button>
    {/each}
  </Sheet>
{/if}

<style>
  .new {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }

  .new input {
    flex: 1;
    min-width: 0;
  }

  .list {
    justify-content: space-between;
    width: 100%;
    margin-top: 6px;
  }
</style>
