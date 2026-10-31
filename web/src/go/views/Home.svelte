<script lang="ts">
  // Where you left off, what you played lately, your playlists, and
  // what's new.
  import { onMount } from 'svelte'
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import { duration } from '../../lib/format'
  import type { GoHome } from '../../lib/types'
  import { carryOn, player } from '../player.svelte'
  import SongList from '../SongList.svelte'

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
  <div class="hero">
    <h1>Your music</h1>
    <p>Pick something to play, or carry on where you left off.</p>
  </div>
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
    <h2>Playlists</h2>
    <div class="chips">
      {#each home.playlists as list (list.id)}
        <a class="chip" href="#/playlist/{list.id}">{list.name}</a>
      {/each}
    </div>
  {/if}

  {#if home.recent.length}
    <h2>Played lately</h2>
    <SongList tracks={home.recent} />
  {/if}

  <h2>New in the library</h2>
  <SongList tracks={home.newest} />
{:else if !error}
  <p class="muted">Loading...</p>
{/if}

<style>
  .hero {
    min-height: 150px;
    padding: 28px 24px;
    border-radius: 16px;
    background: linear-gradient(90deg, rgb(20 17 31 / 0.9), rgb(20 17 31 / 0.3)), url('/images/listening-waves.webp') center / cover;
    color: #ece8f6;
  }

  .hero h1 {
    margin: 0 0 6px;
  }

  .hero p {
    margin: 0;
    max-width: 28ch;
  }

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

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
