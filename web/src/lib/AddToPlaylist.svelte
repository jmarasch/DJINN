<script lang="ts">
  // Add a song, album, or artist to one of your playlists (or a new one).
  import { t } from './i18n.svelte'
  import { api, ApiError } from './api'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import { app, toast, hasPlaylists } from './state.svelte'
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
    if (open) api.playlists().then((list) => (playlists = list.filter((p) => p.owner_id === app.me?.id)))
  })

  function show(e: Event) {
    e.stopPropagation()
    open = true
  }

  async function add(id: number, name: string) {
    try {
      await api.addToPlaylist(id, kind, refId)
      toast(t('atp.added', { name }))
      open = false
    } catch (e) {
      toast(e instanceof ApiError && e.status === 409 ? t('atp.already', { name }) : t('common.add_failed'), true)
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
      toast(err instanceof Error ? err.message : t('pls.create_failed'), true)
    }
  }
</script>

{#if hasPlaylists()}
  {#if !trigger}
    <!-- Opened from elsewhere. -->
  {:else if compact}
    <button class="icon-only" aria-label={t('atp.add_label', { label })} onclick={show}>
      <Icon name="playlist" size={20} />
    </button>
  {:else}
    <button class="pill" onclick={show}><Icon name="playlist" size={18} /> {t('atp.add')}</button>
  {/if}
  {#if open}
    <Sheet title={t('atp.title', { label })} onclose={() => (open = false)}>
      {#each playlists as playlist (playlist.id)}
        <button class="row" onclick={() => add(playlist.id, playlist.name)}>
          <Icon name="playlist" size={20} />
          <span class="ellipsis">{playlist.name}</span>
          {#if playlist.active}<span class="badge">{t('pls.playing')}</span>{/if}
        </button>
      {/each}
      <form class="new" onsubmit={create}>
        <input placeholder={t('atp.new')} bind:value={newName} maxlength="60" />
        <button class="primary" disabled={!newName.trim()}>{t('pls.create')}</button>
      </form>
    </Sheet>
  {/if}
{/if}

<style>
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
