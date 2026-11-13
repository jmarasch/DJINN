<script lang="ts">
  import { t } from '../../lib/i18n.svelte'
  // Where you left off, what you played lately, your playlists, and
  // what's new.
  import { onMount } from 'svelte'
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import PageTitle from '../../lib/PageTitle.svelte'
  import { duration } from '../../lib/format'
  import type { GoHome } from '../../lib/types'
  import { carryOn, player } from '../player.svelte'
  import Discover from '../../lib/Discover.svelte'
  import LikeButton from '../LikeButton.svelte'
  import SongRow from '../SongRow.svelte'
  import SongTile from '../SongTile.svelte'
  import { playList, playRadio, tapSong } from '../player.svelte'
  import type { Track } from '../../lib/types'

  let home = $state<GoHome | null>(null)
  let error = $state('')

  onMount(async () => {
    try {
      home = await api.goHome()
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not load'
    }
  })

  const resumeSong = $derived(
    home?.resume ? home.resume.songs.find((s) => s.id === home!.resume!.queue[home!.resume!.position]) : null,
  )
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if home}
  <PageTitle title={t('go.your_music')} subtitle={t('go.home_hint')} />
  {#if home.resume && resumeSong && player.index < 0}
    {@const r = home.resume}
    <button class="carry-on card" onclick={() => carryOn(r.songs, r.queue, r.position, r.position_ms)}>
      <Art hash={resumeSong.art_hash} size={56} />
      <span class="text">
        <span class="muted small">Carry on where you left off</span>
        <b>{resumeSong.title}</b>
        <span class="muted small">{resumeSong.artist ?? ''} · {duration(r.position_ms)} in</span>
      </span>
    </button>
  {/if}

  {#if home.playlists.length}
    <h2>{t('nav.playlists')}</h2>
    <div class="chips">
      {#each home.playlists as list (list.id)}
        <a class="chip" href="#/playlist/{list.id}">{list.name}</a>
      {/each}
    </div>
  {/if}

  <Discover playAll={(tracks) => playList(tracks, 0)}>
    {#snippet songRow(track: Track)}
      <SongRow {track} onplay={() => tapSong(track, () => playRadio(track))} />
    {/snippet}
    {#snippet songTile(track: Track)}
      <div class="song"><SongTile {track} onplay={() => tapSong(track, () => playRadio(track))} /></div>
    {/snippet}
    {#snippet like(kind: 'artist' | 'album', id: number, name: string)}
      <LikeButton {kind} {id} {name} size={18} />
    {/snippet}
  </Discover>
{:else if !error}
  <p class="muted">Loading...</p>
{/if}

<style>
  .carry-on {
    flex-direction: row;
    align-items: center;
    width: 100%;
    text-align: left;
    color: inherit;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  h2 {
    margin: 20px 0 8px;
    font-size: 1.05rem;
  }

  .song {
    flex: none;
    width: 150px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
