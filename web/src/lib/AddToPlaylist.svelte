<script lang="ts">
  // Admin: add a song, album, or artist to a playlist (or a new one).
  import { api, ApiError } from './api'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { EntryKind, PlaylistListItem } from './types'

  let {
    kind,
    refId,
    label,
    compact = false,
    trigger = true,
    open = $bindable(false),
  }: {
    kind: EntryKind
    refId: number
    label: string
    compact?: boolean
    /** False when something else (a menu) opens it through `open`. */
    trigger?: boolean
    open?: boolean
  } = $props()

  let playlists = $state<PlaylistListItem[]>([])
  let newName = $state('')

  $effect(() => {
    if (open) api.playlists().then((list) => (playlists = list))
  })

  function show(e: Event) {
    e.stopPropagation()
    open = true
  }

  async function add(id: number, name: string) {
    try {
      await api.addToPlaylist(id, kind, refId)
      toast(`Added to ${name}`)
      open = false
    } catch (e) {
      toast(e instanceof ApiError && e.status === 409 ? `Already in ${name}` : 'Could not add it', true)
    }
  }

  async function create(e: SubmitEvent) {
    e.preventDefault()
    const name = newName.trim()
    if (!name) return
    try {
      const playlist = await api.createPlaylist(name)
      newName = ''
      await add(playlist.id, playlist.name)
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not create the playlist', true)
    }
  }
</script>

{#if isAdmin()}
  {#if !trigger}
    <!-- Opened from elsewhere. -->
  {:else if compact}
    <button class="icon-only" aria-label="Add {label} to a playlist" onclick={show}>
      <Icon name="playlist" size={20} />
    </button>
  {:else}
    <button class="add" onclick={show}><Icon name="playlist" size={18} /> Add to playlist</button>
  {/if}
  {#if open}
    <Sheet title="Add {label} to..." onclose={() => (open = false)}>
      {#each playlists as playlist (playlist.id)}
        <button class="row" onclick={() => add(playlist.id, playlist.name)}>
          <Icon name="playlist" size={20} />
          <span class="ellipsis">{playlist.name}</span>
          {#if playlist.active}<span class="badge">Playing</span>{/if}
        </button>
      {/each}
      <form class="new" onsubmit={create}>
        <input placeholder="New playlist name" bind:value={newName} maxlength="60" />
        <button class="primary" disabled={!newName.trim()}>Create</button>
      </form>
    </Sheet>
  {/if}
{/if}

<style>
  .add {
    min-height: 36px;
    margin-top: 10px;
    padding: 0 14px;
    border-radius: 999px;
    font-size: 0.875rem;
  }

  .icon-only {
    flex: none;
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .row {
    width: 100%;
    justify-content: flex-start;
    gap: 12px;
    min-height: 52px;
    padding: 0 4px;
    border-radius: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
  }

  .badge {
    margin-left: auto;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--accent);
    font-size: 0.75rem;
  }

  .new {
    display: flex;
    gap: 8px;
    margin-top: 14px;
  }

  .new input {
    flex: 1;
    min-width: 0;
  }
</style>
