<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import ImportPlaylist from '../lib/ImportPlaylist.svelte'
  import { app, canStartAutoMix, isAdmin, toast } from '../lib/state.svelte'
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
      toast(err instanceof Error ? err.message : t('pls.create_failed'), true)
    }
  }

  async function stop() {
    await api.stopPlaylist().catch((e) => toast(e.message, true))
  }

  async function playWild() {
    if (wild) {
      toast('Wild play is already on')
      return
    }
    try {
      await api.playWild()
      toast('Wild play: random songs from the whole library')
    } catch (e) {
      toast(e instanceof Error ? e.message : t('pls.wild_failed'), true)
    }
  }

  /** Wild play has no playlist of its own: its id is negative. */
  const wild = $derived((app.player.playlist?.id ?? 0) < 0)
</script>

<h1>{t('nav.playlists')}</h1>
<p class="muted small">
  {t('pls.intro_1')} <Icon name="playlist" size={14} /> {t('pls.intro_2')}
  {#if canStartAutoMix()}The playing playlist fills in whenever nobody has requested anything.{/if}
</p>

<a class="favorites card" href="#/favorites">
  <Icon name="heart" />
  <span class="grow">{t('pls.favorites')}</span>
  <span class="small muted">{t('pls.favorites_note')}</span>
</a>

{#if app.player.playlist}
  <div class="playing card">
    <span>{t('pls.playing_from')} <b>{app.player.playlist.name}</b></span>
    {#if canStartAutoMix()}<button onclick={stop}>{t('pls.stop')}</button>{/if}
  </div>
{/if}

{#if isAdmin()}
<button class="row wild" class:active={wild} onclick={playWild}>
  <span class="icon"><Icon name="shuffle" /></span>
  <span class="text">
    <span class="ellipsis block">{t('pls.wild')}</span>
    <span class="muted small">{t('pls.wild_note')}</span>
  </span>
  {#if wild}
    <span class="badge">{t('pls.playing')}</span>
  {:else}
    <span class="play">{t('pls.play')}</span>
  {/if}
</button>
{/if}

<form class="new" onsubmit={create}>
  <input placeholder={t('pls.new')} bind:value={name} maxlength="60" />
  <button class="primary" disabled={!name.trim()}>{t('pls.create')}</button>
</form>
<ImportPlaylist onimported={() => load().catch(() => {})} />

{#each playlists as playlist (playlist.id)}
  <a class="row" href="#/playlist/{playlist.id}">
    <span class="icon"><Icon name="playlist" /></span>
    <span class="text">
      <span class="ellipsis block">{playlist.name}</span>
      <span class="muted small">
        {t('pls.items', { n: playlist.entry_count })} ·
        {playlist.shuffle ? t('pls.shuffle') : t('pls.in_order')}
        {#if playlist.owner_id !== app.me?.id}{t('pl.from', { owner: playlist.owner })}{:else if playlist.shared}{t('pls.shared')}{/if}
        {#if playlist.problem_songs}
          · <span class="flag"><Icon name="warning" size={13} /> {t('pls.cant_play', { n: playlist.problem_songs })}</span>
        {/if}
      </span>
    </span>
    {#if playlist.active}<span class="badge">{t('pls.playing')}</span>{/if}
  </a>
{:else}
  <p class="muted">{t('pls.none')}</p>
{/each}

<style>
  .favorites {
    flex-direction: row;
    align-items: center;
    gap: 10px;
    color: inherit;
    text-decoration: none;
  }

  .favorites :global(svg) {
    color: var(--accent);
  }

  .favorites .grow {
    flex: 1;
    font-weight: 600;
  }

  .wild {
    width: 100%;
    margin-bottom: 12px;
    padding: 0;
    border-radius: 0;
    background: transparent;
    text-align: left;
    font: inherit;
  }

  .wild .play {
    flex: none;
    display: grid;
    place-items: center;
    min-height: 36px;
    padding: 0 16px;
    border-radius: 10px;
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }

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

  .flag {
    color: var(--bad);
    white-space: nowrap;
  }

  .badge {
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--accent);
    font-size: 0.75rem;
  }
</style>
