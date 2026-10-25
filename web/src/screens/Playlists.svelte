<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import { app, toast } from '../lib/state.svelte'
  import type { PlaylistListItem } from '../lib/types'

  let playlists = $state<PlaylistListItem[]>([])
  let name = $state('')

  async function load() {
    playlists = await api.playlists()
  }

  onMount(() => {
    load().catch((e) => toast(e.message, true))
  })

  // Reload when the playing playlist changes.
  $effect(() => {
    void app.player.playlist?.id
    load().catch(() => {})
  })

  async function create(e: SubmitEvent) {
    e.preventDefault()
    try {
      const playlist = await api.createPlaylist(name)
      name = ''
      location.hash = `/playlist/${playlist.id}`
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not create the playlist', true)
    }
  }

  async function stop() {
    await api.stopPlaylist().catch((e) => toast(e.message, true))
  }
</script>

<h1>Playlists</h1>
<p class="muted small">
  The playing playlist fills in whenever nobody has requested anything. Make one per mood or kind of
  party, and add songs, albums, or artists from the Library with the
  <Icon name="playlist" size={14} /> button.
</p>

{#if app.player.playlist}
  <div class="playing card">
    <span>Playing from <b>{app.player.playlist.name}</b></span>
    <button onclick={stop}>Stop</button>
  </div>
{/if}

<form class="new" onsubmit={create}>
  <input placeholder="New playlist, e.g. Chill or Dance" bind:value={name} maxlength="60" />
  <button class="primary" disabled={!name.trim()}>Create</button>
</form>

{#each playlists as playlist (playlist.id)}
  <a class="row" href="#/playlist/{playlist.id}">
    <span class="icon"><Icon name="playlist" /></span>
    <span class="text">
      <span class="ellipsis block">{playlist.name}</span>
      <span class="muted small">
        {playlist.entry_count} {playlist.entry_count === 1 ? 'item' : 'items'} ·
        {playlist.shuffle ? 'shuffle' : 'in order'}
      </span>
    </span>
    {#if playlist.active}<span class="badge">Playing</span>{/if}
  </a>
{:else}
  <p class="muted">No playlists yet. Create one above.</p>
{/each}

<style>
  .playing {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
  }

  .new {
    display: flex;
    gap: 8px;
    margin: 12px 0;
  }

  .new input {
    flex: 1;
    min-width: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 64px;
    color: var(--text);
    text-decoration: none;
    border-bottom: 1px solid var(--line);
  }

  .icon {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--accent);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .block {
    display: block;
  }

  .badge {
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--accent);
    font-size: 0.75rem;
  }
</style>
