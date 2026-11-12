<script lang="ts">
  import { t } from '../lib/i18n.svelte'
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
  import Counts from '../lib/Counts.svelte'
  import JumpStrip from '../lib/JumpStrip.svelte'
  import { watchScroll } from '../lib/scrollWatch.svelte'
  import { jumpCache } from '../lib/listCache'
  import TrackRow from '../lib/TrackRow.svelte'
  import TrackTile from '../lib/TrackTile.svelte'
  import type { AlbumFilter, AlbumSort, ArtistSort, GenreMap, Jump, LyricsFilter, Track, TrackSort } from '../lib/types'

  let {
    song,
    songTile,
    like,
    progress = true,
    stuckBelow,
  }: {
    /** A song in the list view; the party's request row by default. */
    song?: Snippet<[Track]>
    /** A song in the grid view. */
    songTile?: Snippet<[Track]>
    /** A button beside each artist and album (DJINN GO's heart). */
    like?: Snippet<['artist' | 'album', number, string]>
    /** Show the library check's progress at the top. */
    progress?: boolean
    /** What's stuck to the top of the page already, for the options to return under. */
    stuckBelow?: HTMLElement
  } = $props()

  type Tab = 'artists' | 'albums' | 'songs' | 'genres'
  type View = 'list' | 'grid'
  type GenreView = 'tags' | 'cloud'
  const tabs: { id: Tab; label: string }[] = [
    { id: 'artists', label: 'library.artists' },
    { id: 'albums', label: 'library.albums' },
    { id: 'songs', label: 'library.songs' },
    { id: 'genres', label: 'library.genres' },
  ]

  // Tab and sort last for the session; the list/grid choice is kept on
  // this device.
  let tab = $state((sessionStorage.getItem('djinn-browse-tab') as Tab) || 'artists')
  let sort = $state((sessionStorage.getItem('djinn-browse-sort') as TrackSort) || 'title')
  let albumSort = $state((sessionStorage.getItem('djinn-browse-album-sort') as AlbumSort) || 'name')
  let albumFilter = $state<AlbumFilter>('all')
  let lyricsFilter = $state<LyricsFilter>((sessionStorage.getItem('djinn-browse-lyrics') as LyricsFilter) || 'all')
  $effect(() => sessionStorage.setItem('djinn-browse-lyrics', lyricsFilter))
  let artistSort = $state((sessionStorage.getItem('djinn-browse-artist-sort') as ArtistSort) || 'name')
  let view = $state<View>(readView())
  let genreView = $state<GenreView>(readStored('djinn-genre-view') === 'cloud' ? 'cloud' : 'tags')
  let genres = $state<GenreMap | null>(null)
  let genresFailed = $state(false)
  $effect(() => sessionStorage.setItem('djinn-browse-tab', tab))
  $effect(() => sessionStorage.setItem('djinn-browse-sort', sort))
  $effect(() => sessionStorage.setItem('djinn-browse-album-sort', albumSort))
  $effect(() => sessionStorage.setItem('djinn-browse-artist-sort', artistSort))
  $effect(() => store('djinn-browse-view', view))
  $effect(() => store('djinn-genre-view', genreView))

  // The jump strip for the list on show: where each letter or year starts.
  /** Until the search box has been measured. */
  const STUCK_PX = 72
  let list = $state<{ jumpTo: (offset: number) => Promise<void> }>()
  const scroll = watchScroll()
  /** Where the top of the page really starts: below whatever's stuck up there. */
  let stuckTop = $state(STUCK_PX)
  $effect(() => {
    const box = stuckBelow
    if (!box) return
    const measure = () => {
      stuckTop = (parseFloat(getComputedStyle(box).top) || 0) + box.offsetHeight
    }
    measure()
    const watching = new ResizeObserver(measure)
    watching.observe(box)
    addEventListener('resize', measure)
    return () => {
      watching.disconnect()
      removeEventListener('resize', measure)
    }
  })

  async function toTop() {
    scroll.quiet()
    await list?.jumpTo(0)
    scrollTo({ top: 0 })
  }
  let lists = $state<HTMLElement>()
  let jumps = $state<Jump[]>([])
  const jumpKey = $derived(
    tab === 'albums'
      ? `albums-${albumSort}-${albumFilter}`
      : tab === 'songs'
        ? `songs-${sort}-${lyricsFilter}`
        : tab === 'artists'
          ? `artists-${artistSort}`
          : tab,
  )
  const byYear = $derived(
    (tab === 'albums' && (albumSort === 'year' || albumSort === 'oldest')) ||
      (tab === 'songs' && (sort === 'year' || sort === 'oldest')),
  )
  const newestFirst = $derived((tab === 'albums' ? albumSort : sort) === 'year')
  $effect(() => {
    const k = jumpKey
    const cached = jumpCache.get(k)
    jumps = cached ?? []
    if (cached || tab === 'genres') return
    const loading =
      tab === 'artists' ? api.artistJumps(artistSort) : tab === 'albums' ? api.albumJumps(albumSort, albumFilter) : api.trackJumps(sort, lyricsFilter)
    loading
      .then((found) => {
        jumpCache.set(k, found)
        if (k === jumpKey) jumps = found
      })
      .catch(() => {
        // No strip; the list still scrolls.
      })
  })
  /** The jump strip had no room beside the list and sits over its edge. */
  let stripInside = $state(false)
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
  /** "12 songs" in the reader's language: `kind` is a count key. */
  const plural = (n: number, kind: string) => t(`count.${kind}`, { n })
  const initial = (name: string) => name.charAt(0).toUpperCase()
</script>

{#if progress}<LibraryProgress />{/if}

<div
  class="controls"
  class:returning={scroll.up}
  style:top={scroll.up ? `${stuckTop}px` : null}
>
  <div class="bar">
    <div class="tabs" role="tablist">
      {#each tabs as item (item.id)}
        <button role="tab" aria-selected={tab === item.id} class:active={tab === item.id} onclick={() => (tab = item.id)}>
          {t(item.label)}
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
  {#if tab === 'artists'}
    <label class="sort">
        <span class="muted small">{t('library.sort_by')}</span>
        <select bind:value={artistSort}>
          <option value="name">{t('library.sort_name')}</option>
          <option value="added">{t('library.sort_added')}</option>
        </select>
      </label>
  {:else if tab === 'albums'}
    <div class="album-filters">
        <label class="sort">
          <span class="muted small">{t('library.show')}</span>
          <select bind:value={albumFilter}>
            <option value="all">{t('library.all_albums')}</option>
            <option value="various">{t('library.various')}</option>
            <option value="soundtracks">{t('library.soundtracks')}</option>
          </select>
        </label>
        <label class="sort">
          <span class="muted small">{t('library.sort_by')}</span>
          <select bind:value={albumSort}>
            <option value="name">{t('library.sort_name')}</option>
            <option value="added">{t('library.sort_added')}</option>
            <option value="year">{t('library.sort_newest')}</option>
            <option value="oldest">{t('library.sort_oldest')}</option>
          </select>
        </label>
      </div>
  {:else if tab === 'songs'}
    <div class="album-filters">
      <label class="sort">
        <span class="muted small">{t('library.show')}</span>
        <select bind:value={lyricsFilter}>
          <option value="all">{t('library.all_songs')}</option>
          <option value="words">{t('library.with_lyrics')}</option>
          <option value="timed">{t('library.with_timed_lyrics')}</option>
        </select>
      </label>
    <label class="sort">
        <span class="muted small">{t('library.sort_by')}</span>
        <select bind:value={sort}>
          <option value="title">{t('library.sort_title')}</option>
          <option value="artist">{t('library.sort_artist')}</option>
          <option value="added">{t('library.sort_added')}</option>
          <option value="popular">{t('library.sort_popular')}</option>
          <option value="year">{t('library.sort_newest')}</option>
          <option value="oldest">{t('library.sort_oldest')}</option>
        </select>
      </label>
    </div>
  {/if}
</div>

{#if indexed}
  <JumpStrip
    {jumps}
    years={byYear}
    {newestFirst}
    below={lists}
    bind:inside={stripInside}
    onjump={(offset) => {
      // Landing where it was asked to, not under the options.
      scroll.quiet()
      list?.jumpTo(offset)
    }}
  />
{/if}

{#if scroll.up && scroll.far}
  <button class="to-top" aria-label="Back to top" onclick={toTop}>
    <Icon name="up" size={24} />
  </button>
{/if}

<div class="lists" class:crowded={indexed && stripInside} bind:this={lists}>
  {#if tab === 'artists'}
        <InfiniteList
      bind:this={list}
      inset={stuckTop}
      cacheKey="artists-{artistSort}"
      load={(offset) => api.artists(artistSort, offset)}
      key={(a) => a.id}
      count={(n) => plural(n, 'artists')}
      {grid}
      empty="No artists yet. Rescan the library from Admin after adding music."
    >
      {#snippet item(artist)}
        {#if grid}
          <div class="with-like">
            <a class="tile" href="#/artist/{artist.id}">
              <div class="cover"><Art hash={artist.art_hash} size={160} round={999} letter={initial(artist.name)} /></div>
              <span class="ellipsis center-text">{artist.name}</span>
              <span class="muted small center-text"
                >{plural(artist.track_count, 'songs')}<Counts plays={artist.plays} likes={artist.likes} /></span
              >
            </a>
            {#if like}<div class="tile-like">{@render like('artist', artist.id, artist.name)}</div>{/if}
          </div>
        {:else}
          <div class="row-with-like">
            <a class="row" href="#/artist/{artist.id}">
              <Art hash={artist.art_hash} size={44} round={999} letter={initial(artist.name)} />
              <span class="name ellipsis">{artist.name}</span>
              <span class="muted small counts"
                >{plural(artist.track_count, 'songs')}<Counts plays={artist.plays} likes={artist.likes} /></span
              >
            </a>
            {#if like}{@render like('artist', artist.id, artist.name)}{/if}
          </div>
        {/if}
      {/snippet}
    </InfiniteList>
  {:else if tab === 'genres'}
    {#if genres === null}
      <p class="muted small">{genresFailed ? t('library.genres_failed') : t('library.genres_loading')}</p>
    {:else if genres.genres.length === 0}
      <p class="muted">
        No genres yet. They come from your songs' tags, MusicBrainz lookups (Admin > Rules), and AI listening (Admin >
        AI).
      </p>
    {:else}
      {#if genreView === 'cloud'}
        <GenreCloud />
      {:else}
        <p class="muted small">{plural(genres.genres.length, 'genres')}</p>
        <GenreTags map={genres} />
      {/if}
    {/if}
  {:else if tab === 'albums'}
        <InfiniteList
      bind:this={list}
      inset={stuckTop}
      cacheKey="albums-{albumSort}-{albumFilter}"
      load={(offset) => api.albums(albumSort, albumFilter, offset)}
      key={(a) => a.id}
      count={(n) => plural(n, 'albums')}
      {grid}
      empty="No albums. Songs without an album tag are under Artists and Songs."
    >
      {#snippet item(album)}
        {#if grid}
          <div class="with-like">
            <a class="tile" href="#/album/{album.id}">
              <div class="cover"><Art hash={album.art_hash} size={160} round={10} /></div>
              <span class="ellipsis">{album.title}</span>
              <span class="muted small ellipsis"
                >{album.artist}{byYear && album.year ? ` · ${album.year}` : ''}<Counts
                  plays={album.plays}
                  likes={album.likes}
                /></span
              >
            </a>
            {#if like}<div class="tile-like">{@render like('album', album.id, album.title)}</div>{/if}
          </div>
        {:else}
          <div class="row-with-like">
            <a class="row" href="#/album/{album.id}">
              <Art hash={album.art_hash} size={52} />
              <span class="name">
                <span class="ellipsis block">{album.title}</span>
                <span class="muted small ellipsis block">
                  {album.artist}{album.year ? ` · ${album.year}` : ''}<Counts plays={album.plays} likes={album.likes} />
                </span>
              </span>
            </a>
            {#if like}{@render like('album', album.id, album.title)}{/if}
          </div>
        {/if}
      {/snippet}
    </InfiniteList>
  {:else}
        <InfiniteList
      bind:this={list}
      inset={stuckTop}
      cacheKey="songs-{sort}-{lyricsFilter}"
      load={(offset) => api.tracks(sort, offset, lyricsFilter)}
      key={(t) => t.id}
      count={(n) => plural(n, 'songs')}
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
    border: 1px solid var(--edge);
    border-radius: 14px;
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
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
    box-shadow: var(--glow-strong);
  }

  /* An artist's song and play counts stay on one line beside the name. */
  .counts {
    flex: none;
    white-space: nowrap;
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

  .row-with-like {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--line);
  }

  .row-with-like .row {
    flex: 1;
    min-width: 0;
    border-bottom: none;
  }

  .with-like {
    position: relative;
    min-width: 0;
  }

  /* The heart sits on the cover's corner. */
  .tile-like {
    position: absolute;
    top: 4px;
    right: 4px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.45);
  }

  .tile-like :global(button:not(.liked)) {
    color: #fff;
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

  /* Square whatever the picture's shape: the list assumes every row is the
     same height. Without overflow hidden, a tall picture stretches the box
     past its aspect ratio. */
  .cover {
    aspect-ratio: 1;
    min-height: 0;
    overflow: hidden;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  /* Room for the jump strip on the right, when it's over the list rather
     than beside it. */
  .lists.crowded {
    padding-right: 18px;
  }

  /* In the page's flow, so the list doesn't move when they come back. */
  .controls {
    padding-bottom: 8px;
    background: var(--bg);
  }

  .controls.returning {
    position: sticky;
    z-index: 4;
    animation: slide-in 0.15s ease-out;
  }

  @keyframes slide-in {
    from {
      transform: translateY(-16px);
      opacity: 0;
    }
  }

  .to-top {
    position: fixed;
    right: 32px;
    bottom: calc(var(--clear-of-player, 0px) + 24px + env(safe-area-inset-bottom));
    z-index: 6;
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    padding: 0;
    border-radius: 999px;
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.4);
  }

  .sort {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 10px;
    margin-bottom: 4px;
  }

  .album-filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 16px;
    margin-bottom: 4px;
  }

  .album-filters .sort { margin-bottom: 0; }
</style>
