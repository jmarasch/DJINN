<script lang="ts">
  // One genre: its songs, artists and related genres.
  import { api } from '../../lib/api'
  import Icon from '../../lib/Icon.svelte'
  import type { GenrePage } from '../../lib/types'
  import { playList, setShuffle } from '../player.svelte'
  import SongList from '../SongList.svelte'

  let { name }: { name: string } = $props()
  let page = $state<GenrePage | null>(null)
  let error = $state('')

  $effect(() => {
    page = null
    error = ''
    api
      .genre(name)
      .then((p) => (page = p))
      .catch((e) => (error = e.message))
  })

  const title = $derived(name.charAt(0).toUpperCase() + name.slice(1))

  function shuffleAll() {
    if (!page?.tracks.length) return
    setShuffle(true)
    playList(page.tracks, Math.floor(Math.random() * page.tracks.length))
  }
</script>

<h1>{title}</h1>
{#if error}
  <p class="error">{error}</p>
{:else if page}
  <p class="small muted">
    {page.total}
    {page.total === 1 ? 'song' : 'songs'}{page.total > page.tracks.length ? `; the ${page.tracks.length} most played here` : ''}.
  </p>
  <div class="buttons">
    <button class="primary" onclick={() => page && playList(page.tracks)} disabled={!page.tracks.length}>
      <Icon name="play" /> Play
    </button>
    <button onclick={shuffleAll} disabled={!page.tracks.length}><Icon name="shuffle" /> Shuffle</button>
  </div>
  {#if page.related.length}
    <h2>Related genres</h2>
    <div class="chips">
      {#each page.related as genre (genre.name)}
        <a class="chip" href="#/genre/{encodeURIComponent(genre.name)}">{genre.name}</a>
      {/each}
    </div>
  {/if}
  {#if page.artists.length}
    <h2>Artists</h2>
    <div class="chips">
      {#each page.artists as artist (artist.id)}
        <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
      {/each}
    </div>
  {/if}
  <h2>Songs</h2>
  <SongList tracks={page.tracks} />
{:else}
  <p class="muted">Loading...</p>
{/if}

<style>
  h1 {
    margin: 0;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
