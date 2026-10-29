<script lang="ts">
  import { api } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import RadioButton from '../lib/RadioButton.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { GenrePage } from '../lib/types'

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
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

<h1>{title}</h1>
{#if error}
  <p class="error">{error}</p>
{:else if page}
  <p class="small muted">
    {page.total}
    {page.total === 1 ? 'song' : 'songs'}, by their tags or how they sound{page.total > page.tracks.length
      ? `; the ${page.tracks.length} most played first`
      : ''}.
  </p>
  <RadioButton seed={{ kind: 'genre', name: page.name }} label="{title} radio" />
  {#if page.related.length}
    <h2>Related genres</h2>
    <div class="chips">
      {#each page.related as genre (genre.name)}
        <a
          class="chip related"
          href="#/genre/{encodeURIComponent(genre.name)}"
          style:--strength={genre.strength}
          title="{genre.songs} {genre.songs === 1 ? 'song' : 'songs'}"
        >
          {genre.name}
          <span class="count">{genre.songs}</span>
        </a>
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
  {#if page.tracks.length}
    <h2>Songs</h2>
    {#each page.tracks as track (track.id)}
      <TrackRow {track} />
    {/each}
  {:else}
    <p class="muted">No songs with this genre yet.</p>
  {/if}
{:else}
  <p class="muted">Loading...</p>
{/if}

<style>
  /* The more related, the stronger the outline, like the lines on the
     genre cloud. */
  .related {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    box-shadow: inset 0 0 0 calc(1px + 1.5px * var(--strength))
      color-mix(in srgb, var(--accent) calc(25% + 60% * var(--strength)), transparent);
  }

  .count {
    color: var(--muted);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }

  h2 {
    margin-top: 18px;
  }
</style>
