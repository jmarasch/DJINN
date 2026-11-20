<script lang="ts">
  // Picks one of your playlists (or makes a new one) for songs.
  import { t } from '../lib/i18n.svelte'
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
  const songs = () => (ui.adding ?? []).map((track) => track.id)
  const what = () =>
    count() === 1 ? t('gp2.song_q', { title: ui.adding![0].title }) : t('count.songs', { n: count() })

  async function add(list: GoPlaylist) {
    const label = what()
    try {
      await api.goAddToPlaylist(list.id, songs())
      toast(t('gat.added', { what: label, name: list.name }))
      ui.adding = null
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gat.add_failed'), true)
    }
  }

  async function create(e: SubmitEvent) {
    e.preventDefault()
    const label = what()
    try {
      const list = await api.goCreatePlaylist(name, songs())
      toast(t('gat.made', { name: list.name, what: label }))
      name = ''
      ui.adding = null
    } catch (err) {
      toast(err instanceof Error ? err.message : t('gl.make_failed'), true)
    }
  }
</script>

{#if ui.adding}
  <Sheet title={t('gat.title')} onclose={() => (ui.adding = null)}>
    <form class="new" onsubmit={create}>
      <input bind:value={name} placeholder={t('gl.new')} maxlength="100" required aria-label={t('atp.new')} />
      <button class="primary">{t('gl.make')}</button>
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
