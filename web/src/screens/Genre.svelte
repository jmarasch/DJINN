<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import InfiniteList from '../lib/InfiniteList.svelte'
  import RadioButton from '../lib/RadioButton.svelte'
  import ScrollRow from '../lib/ScrollRow.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import type { ArtistSummary, GenrePage } from '../lib/types'

  let { name }: { name: string } = $props()
  let page = $state<GenrePage | null>(null)
  let error = $state('')
  let artists = $state<ArtistSummary[]>([])
  let fetchingArtists = false

  $effect(() => {
    page = null
    error = ''
    artists = []
    api
      .genre(name)
      .then((p) => {
        page = p
        artists = p.artists
      })
      .catch((e) => (error = e.message))
  })

  async function moreArtists() {
    if (fetchingArtists || !page || artists.length >= page.artist_total) return
    fetchingArtists = true
    try {
      const next = await api.genreArtists(name, artists.length)
      if (next.items.length) artists = [...artists, ...next.items]
    } finally {
      fetchingArtists = false
    }
  }

  const title = $derived(name.charAt(0).toUpperCase() + name.slice(1))
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

<h1>{title}</h1>
{#if error}
  <p class="error">{error}</p>
{:else if page}
  <p class="small muted">
    {page.total}
    {page.total === 1 ? 'song' : 'songs'}, by their tags or how they sound, shuffled with the most played nearer the top.
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
  {#if artists.length}
    <h2>Artists <span class="muted small">{page.artist_total}</span></h2>
    <ScrollRow nearEnd={moreArtists}>
      {#each artists as artist (artist.id)}
        <a class="artist" href="#/artist/{artist.id}">
          <Art hash={artist.art_hash} size={96} round={999} letter={artist.name.charAt(0).toUpperCase()} />
          <span class="ellipsis">{artist.name}</span>
        </a>
      {/each}
    </ScrollRow>
  {/if}
  {#if page.tracks.length}
    <h2>Songs</h2>
    <InfiniteList cacheKey="genre-{name}" load={(offset) => api.genreSongs(name, offset)} key={(t) => t.id}>
      {#snippet item(track)}
        <TrackRow {track} />
      {/snippet}
    </InfiniteList>
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

  .artist {
    flex: none;
    width: 96px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: var(--text);
    text-decoration: none;
    font-size: 0.8125rem;
    text-align: center;
  }

  .artist .ellipsis {
    max-width: 100%;
  }
</style>
