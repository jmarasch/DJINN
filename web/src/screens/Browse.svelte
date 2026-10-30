<script lang="ts">
  // The library by artist, album, song or genre. The party app's songs
  // queue at the party; DJINN GO passes its own `song` and `songTile`.
  import type { Snippet } from 'svelte'
  import LibraryProgress from '../lib/LibraryProgress.svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import GenreCloud from '../lib/GenreCloud.svelte'
  import GenreTags from '../lib/GenreTags.svelte'
  import Icon from '../lib/Icon.svelte'
  import InfiniteList from '../lib/InfiniteList.svelte'
  import JumpStrip from '../lib/JumpStrip.svelte'
  import { jumpCache } from '../lib/listCache'
  import TrackRow from '../lib/TrackRow.svelte'
  import TrackTile from '../lib/TrackTile.svelte'
  import type { AlbumSort, GenreMap, Jump, Track, TrackSort } from '../lib/types'

  let {
    song,
    songTile,
    progress = true,
  }: {
    /** A song in the list view; the party's request row by default. */
    song?: Snippet<[Track]>
    /** A song in the grid view. */
    songTile?: Snippet<[Track]>
    /** Show the library check's progress at the top. */
    progress?: boolean
  } = $props()

  type Tab = 'artists' | 'albums' | 'songs' | 'genres'
  type View = 'list' | 'grid'
  type GenreView = 'tags' | 'cloud'
  const tabs: { id: Tab; label: string }[] = [
    { id: 'artists', label: 'Artists' },
    { id: 'albums', label: 'Albums' },
    { id: 'songs', label: 'Songs' },
    { id: 'genres', label: 'Genres' },
  ]

  // Tab and sort last for the session; the list/grid choice is kept on
  // this device.
  let tab = $state((sessionStorage.getItem('djinn-browse-tab') as Tab) || 'artists')
  let sort = $state((sessionStorage.getItem('djinn-browse-sort') as TrackSort) || 'title')
  let albumSort = $state((sessionStorage.getItem('djinn-browse-album-sort') as AlbumSort) || 'name')
  let view = $state<View>(readView())
  let genreView = $state<GenreView>(readStored('djinn-genre-view') === 'cloud' ? 'cloud' : 'tags')
  let genres = $state<GenreMap | null>(null)
  let genresFailed = $state(false)
  $effect(() => sessionStorage.setItem('djinn-browse-tab', tab))
  $effect(() => sessionStorage.setItem('djinn-browse-sort', sort))
  $effect(() => sessionStorage.setItem('djinn-browse-album-sort', albumSort))
  $effect(() => store('djinn-browse-view', view))
  $effect(() => store('djinn-genre-view', genreView))

  // The jump strip for the list on show: where each letter or year starts.
  /** Height of the search box stuck to the top, which jumps land below. */
  const STUCK_PX = 72
  let list = $state<{ jumpTo: (offset: number) => void }>()
  let lists = $state<HTMLElement>()
  let jumps = $state<Jump[]>([])
  const jumpKey = $derived(tab === 'albums' ? `albums-${albumSort}` : tab === 'songs' ? `songs-${sort}` : tab)
  const byYear = $derived(
    (tab === 'albums' && albumSort !== 'name') || (tab === 'songs' && (sort === 'year' || sort === 'oldest')),
  )
  const newestFirst = $derived((tab === 'albums' ? albumSort : sort) === 'year')
  $effect(() => {
    const k = jumpKey
    const cached = jumpCache.get(k)
    jumps = cached ?? []
    if (cached || tab === 'genres') return
    const loading =
      tab === 'artists' ? api.artistJumps() : tab === 'albums' ? api.albumJumps(albumSort) : api.trackJumps(sort)
    loading
      .then((found) => {
        jumpCache.set(k, found)
        if (k === jumpKey) jumps = found
      })
      .catch(() => {
        // No strip; the list still scrolls.
      })
  })
  const indexed = $derived(tab !== 'genres' && jumps.length > 1)

  // The genres load the first time their tab opens.
  $effect(() => {
    if (tab !== 'genres' || genres) return
    api
      .genres()
      .then((map) => (genres = map))
      .catch(() => (genresFailed = true))
  })

  function readView(): View {
    return readStored('djinn-browse-view') === 'grid' ? 'grid' : 'list'
  }

  function readStored(key: string): string | null {
    try {
      return localStorage.getItem(key)
    } catch {
      return null
    }
  }

  function store(key: string, value: string) {
    try {
      localStorage.setItem(key, value)
    } catch {
      // Storage unavailable (private mode); the choice just isn't kept.
    }
  }

  const grid = $derived(view === 'grid')
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`
  const initial = (name: string) => name.charAt(0).toUpperCase()
</script>

{#if progress}<LibraryProgress />{/if}

<div class="bar">
  <div class="tabs" role="tablist">
    {#each tabs as t (t.id)}
      <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => (tab = t.id)}>
        {t.label}
      </button>
    {/each}
  </div>
  <div class="views" role="group" aria-label="Layout">
    {#if tab === 'genres'}
      <button
        class:active={genreView === 'tags'}
        aria-label="Tags"
        aria-pressed={genreView === 'tags'}
        onclick={() => (genreView = 'tags')}
      >
        <Icon name="list" size={20} />
      </button>
      <button
        class:active={genreView === 'cloud'}
        aria-label="Cloud"
        aria-pressed={genreView === 'cloud'}
        onclick={() => (genreView = 'cloud')}
      >
        <Icon name="cloud" size={20} />
      </button>
    {:else}
      <button class:active={!grid} aria-label="List" aria-pressed={!grid} onclick={() => (view = 'list')}>
        <Icon name="list" size={20} />
      </button>
      <button class:active={grid} aria-label="Grid" aria-pressed={grid} onclick={() => (view = 'grid')}>
        <Icon name="grid" size={20} />
      </button>
    {/if}
  </div>
</div>

{#if indexed}
  <JumpStrip {jumps} years={byYear} {newestFirst} below={lists} onjump={(offset) => list?.jumpTo(offset)} />
{/if}

<div class="lists" class:indexed bind:this={lists}>
  {#if tab === 'artists'}
    <InfiniteList
      bind:this={list}
      inset={STUCK_PX}
      cacheKey="artists"
      load={api.artists}
      key={(a) => a.id}
      count={(n) => plural(n, 'artist', 'artists')}
      {grid}
      empty="No artists yet. Rescan the library from Admin after adding music."
    >
      {#snippet item(artist)}
        {#if grid}
          <a class="tile" href="#/artist/{artist.id}">
            <div class="cover"><Art hash={artist.art_hash} size={160} round={999} letter={initial(artist.name)} /></div>
            <span class="ellipsis center-text">{artist.name}</span>
            <span class="muted small center-text">{plural(artist.track_count, 'song', 'songs')}</span>
          </a>
        {:else}
          <a class="row" href="#/artist/{artist.id}">
            <Art hash={artist.art_hash} size={44} round={999} letter={initial(artist.name)} />
            <span class="name ellipsis">{artist.name}</span>
            <span class="muted small">{plural(artist.track_count, 'song', 'songs')}</span>
          </a>
        {/if}
      {/snippet}
    </InfiniteList>
  {:else if tab === 'genres'}
    {#if genres === null}
      <p class="muted small">{genresFailed ? 'Could not load the genres.' : 'Loading genres...'}</p>
    {:else if genres.genres.length === 0}
      <p class="muted">
        No genres yet. They come from your songs' tags, MusicBrainz lookups (Admin > Rules), and AI listening (Admin >
        AI).
      </p>
    {:else}
      <p class="muted small">
        {plural(genres.genres.length, 'genre', 'genres')}{genreView === 'cloud'
          ? ' · bigger means more songs, lines join related genres'
          : ''}
      </p>
      {#if genreView === 'cloud'}
        <GenreCloud map={genres} />
      {:else}
        <GenreTags map={genres} />
      {/if}
    {/if}
  {:else if tab === 'albums'}
    <label class="sort">
      <span class="muted small">Sort by</span>
      <select bind:value={albumSort}>
        <option value="name">Name</option>
        <option value="year">Year (newest first)</option>
        <option value="oldest">Year (oldest first)</option>
      </select>
    </label>
    <InfiniteList
      bind:this={list}
      inset={STUCK_PX}
      cacheKey="albums-{albumSort}"
      load={(offset) => api.albums(albumSort, offset)}
      key={(a) => a.id}
      count={(n) => plural(n, 'album', 'albums')}
      {grid}
      empty="No albums. Songs without an album tag are under Artists and Songs."
    >
      {#snippet item(album)}
        {#if grid}
          <a class="tile" href="#/album/{album.id}">
            <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
            <span class="ellipsis">{album.title}</span>
            <span class="muted small ellipsis">{album.artist}{albumSort !== 'name' && album.year ? ` · ${album.year}` : ''}</span>
          </a>
        {:else}
          <a class="row" href="#/album/{album.id}">
            <Art hash={album.art_hash} size={52} />
            <span class="name">
              <span class="ellipsis block">{album.title}</span>
              <span class="muted small ellipsis block">
                {album.artist}{album.year ? ` · ${album.year}` : ''}
              </span>
            </span>
          </a>
        {/if}
      {/snippet}
    </InfiniteList>
  {:else}
    <label class="sort">
      <span class="muted small">Sort by</span>
      <select bind:value={sort}>
        <option value="title">Title</option>
        <option value="artist">Artist</option>
        <option value="added">Recently added</option>
        <option value="popular">Most popular</option>
        <option value="year">Year (newest first)</option>
        <option value="oldest">Year (oldest first)</option>
      </select>
    </label>
    <InfiniteList
      bind:this={list}
      inset={STUCK_PX}
      cacheKey="songs-{sort}"
      load={(offset) => api.tracks(sort, offset)}
      key={(t) => t.id}
      count={(n) => plural(n, 'song', 'songs')}
      {grid}
    >
      {#snippet item(track)}
        {#if grid}
          {#if songTile}{@render songTile(track)}{:else}<TrackTile {track} />{/if}
        {:else if song}
          {@render song(track)}
        {:else}
          <TrackRow {track} note={byYear && track.year ? String(track.year) : null} />
        {/if}
      {/snippet}
    </InfiniteList>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    gap: 8px;
    margin: 4px 0 8px;
  }

  .tabs,
  .views {
    display: flex;
    gap: 4px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
  }

  .tabs {
    flex: 1;
    min-width: 0;
  }

  .tabs button,
  .views button {
    min-height: 40px;
    background: transparent;
    color: var(--muted);
  }

  .tabs button {
    flex: 1;
    min-width: 0;
    padding: 0 4px;
  }

  /* Four tabs and the layout buttons fit on a small phone. */
  @media (max-width: 400px) {
    .tabs button {
      font-size: 0.9375rem;
    }
  }

  .views button {
    width: 40px;
    padding: 0;
  }

  .tabs button.active,
  .views button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 62px;
    color: var(--text);
    text-decoration: none;
    border-bottom: 1px solid var(--line);
  }

  .name {
    flex: 1;
    min-width: 0;
  }

  .block {
    display: block;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }

  .center-text {
    text-align: center;
  }

  .cover {
    aspect-ratio: 1;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  /* Room for the jump strip on the right, when it's on the page rather
     than beside it. */
  @media (max-width: 720px) {
    .lists.indexed {
      padding-right: 18px;
    }
  }

  .sort {
    flex-direction: row;
    align-items: center;
    gap: 10px;
    margin-bottom: 4px;
  }
</style>
