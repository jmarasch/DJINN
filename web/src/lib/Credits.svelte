<script lang="ts">
  // A song's artists, each linked: "A & B feat. C".
  import type { Track } from './types'

  let { track }: { track: Pick<Track, 'artist' | 'artist_id' | 'credits'> } = $props()

  const main = $derived(track.credits.filter((c) => !c.featured))
  const featured = $derived(track.credits.filter((c) => c.featured))
  /** ", " between names and " & " before the last. */
  const joiner = (i: number, count: number) => (i === 0 ? '' : i === count - 1 ? ' & ' : ', ')
</script>

{#if track.credits.length}
  {#each main as credit, i (credit.id)}{joiner(i, main.length)}<a href="#/artist/{credit.id}">{credit.name}</a>{/each}
  {#if featured.length}
    <span class="feat">feat.</span>
    {#each featured as credit, i (credit.id)}{joiner(i, featured.length)}<a href="#/artist/{credit.id}">{credit.name}</a
      >{/each}
  {/if}
{:else if track.artist_id}
  <a href="#/artist/{track.artist_id}">{track.artist}</a>
{:else}
  {track.artist ?? ''}
{/if}

<style>
  a {
    color: inherit;
    text-decoration: none;
  }

  a:hover {
    text-decoration: underline;
  }

  .feat {
    opacity: 0.8;
  }
</style>
