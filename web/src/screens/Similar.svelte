<script lang="ts">
  // Songs that sound like one song.
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Credits from '../lib/Credits.svelte'
  import Icon from '../lib/Icon.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { Track } from '../lib/types'

  let { id }: { id: number } = $props()
  let seed = $state<Track | null>(null)
  let similar = $state<Track[] | null>(null)
  let analyzed = $state(true)
  let error = $state('')

  $effect(() => {
    seed = null
    similar = null
    Promise.all([api.track(id), api.similarTracks(id)])
      .then(([track, result]) => {
        seed = track
        similar = result.tracks
        analyzed = result.analyzed
      })
      .catch((e) => (error = e.message))
  })
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

{#if error}
  <p class="error">{error}</p>
{:else if seed && similar}
  <header>
    <Art hash={seed.art_hash} size={72} round={10} />
    <div class="grow">
      <div class="muted small">More like</div>
      <h1 class="ellipsis">{seed.title}</h1>
      <div class="muted ellipsis"><Credits track={seed} /></div>
    </div>
  </header>
  {#if !analyzed}
    <p class="muted">
      DJINN hasn't listened to this song yet. It analyzes new songs in the background, so check back in a little
      while.
    </p>
  {:else}
    <p class="small muted">Songs that sound alike: tempo, energy, tone, and mood.</p>
    {#each similar as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
{:else}
  <p class="muted">Listening...</p>
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 8px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  h1 {
    margin: 2px 0;
    font-size: 1.375rem;
  }
</style>
