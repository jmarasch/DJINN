<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Browse from './Browse.svelte'
  import Icon from '../lib/Icon.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import { isAdmin, toast } from '../lib/state.svelte'
  import type { SearchResults } from '../lib/types'

  // Remember the last search when coming back from an album or artist.
  let query = $state(sessionStorage.getItem('djinn-search') ?? '')
  let input = $state<HTMLInputElement>()
  let results = $state<SearchResults | null>(null)
  let loading = $state(false)
  /** The AI reader is reading the query. */
  let thinking = $state(false)
  let latest = 0

  const musings = [
    'Thinking...',
    'Digging through the crates...',
    'Flipping through records...',
    'Asking the DJ...',
    'Reading the room...',
    'Checking the back catalog...',
    'Cueing up ideas...',
  ]
  let musing = $state(musings[0])

  // A new phrase every couple of seconds while the reader works.
  $effect(() => {
    if (!thinking) return
    let at = Math.floor(Math.random() * musings.length)
    musing = musings[at]
    const timer = setInterval(() => {
      at = (at + 1 + Math.floor(Math.random() * (musings.length - 1))) % musings.length
      musing = musings[at]
    }, 2200)
    return () => clearInterval(timer)
  })

  $effect(() => {
    const q = query.trim()
    sessionStorage.setItem('djinn-search', query)
    if (!q) {
      results = null
      return
    }
    const id = ++latest
    thinking = false
    const timer = setTimeout(async () => {
      loading = true
      try {
        const found = await api.search(q)
        if (id !== latest) return
        results = found
        loading = false
        if (!found.ask) return
        // Words the phrase reader didn't know: once typing stops, let the
        // AI reader have a go (a few seconds).
        thinking = true
        await new Promise((resolve) => setTimeout(resolve, 600))
        if (id !== latest) return
        const read = await api.ask(q)
        if (id === latest && read && results) results = { ...results, described: read }
      } catch {
        // Keep what was found.
      } finally {
        if (id === latest) {
          loading = false
          thinking = false
        }
      }
    }, 150)
    return () => clearTimeout(timer)
  })

  const described = $derived(results?.described ?? null)
  const empty = $derived(
    results &&
      !results.tracks.length &&
      !results.albums.length &&
      !results.artists.length &&
      !described?.tracks.length,
  )

  /** Plain matches by name come first when there are any; a description
   * with nothing by that name leads with what fits. */
  const describedFirst = $derived(!!described && !results?.tracks.length && !results?.artists.length)

  async function savePlaylist() {
    if (!described) return
    const label = described.summary.join(' ')
    const name = label.charAt(0).toUpperCase() + label.slice(1)
    try {
      const playlist = await api.playlistFromSongs(name, described.tracks.map((t) => t.id))
      toast(`Made the playlist ${playlist.name}`)
      location.hash = `/playlist/${playlist.id}`
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not make the playlist', true)
    }
  }
</script>

{#snippet describedSection()}
  {#if thinking}
    <p class="small muted thinking"><span class="spinner" aria-hidden="true"></span> {musing}</p>
  {/if}
  {#if described}
    <div class="described">
      <div class="described-head">
        <div>
          <h2>
            Matches <span class="muted">{described.summary.join(' · ')}</span>
            {#if described.read_by_ai}<span class="tag">read by AI</span>{/if}
          </h2>
          {#if described.tracks.length}
            <p class="small muted">
              {described.total} {described.total === 1 ? 'song fits' : 'songs fit'}{described.total >
              described.tracks.length
                ? `, showing ${described.tracks.length}`
                : ''}.{described.unknown.length ? ` Not sure about "${described.unknown.join(' ')}".` : ''}
            </p>
          {:else}
            <p class="small muted">
              Nothing in the library fits that.{#if isAdmin()}
                Genres come from your files' tags and from MusicBrainz (Admin > Rules), so songs without either can't
                match a genre.{/if}
            </p>
          {/if}
        </div>
        {#if isAdmin() && described.tracks.length}
          <button class="small-button" onclick={savePlaylist}>Save as playlist</button>
        {/if}
      </div>
      {#if described.artists.length}
        <div class="chips">
          {#each described.artists as artist (artist.id)}
            <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
          {/each}
        </div>
      {/if}
      {#each described.tracks as track (track.id)}
        <TrackRow {track} />
      {/each}
    </div>
  {/if}
{/snippet}

<div class="search-box">
  <Icon name="search" />
  <div class="field">
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="search"
      placeholder="Search, or describe: 80s rock, chill house..."
      bind:value={query}
      bind:this={input}
      autofocus
      autocomplete="off"
      autocapitalize="none"
      enterkeyhint="search"
    />
    {#if query}
      <button
        class="clear"
        aria-label="Clear the search"
        onclick={() => {
          query = ''
          input?.focus()
        }}
      >
        <Icon name="close" size={20} />
      </button>
    {/if}
  </div>
</div>

{#if empty && !loading && !thinking && !described}
  <p class="muted center">No matches for "{query.trim()}".</p>
{/if}

{#if results}
  {#if describedFirst}{@render describedSection()}{/if}
  {#if results.artists.length}
    <h2>Artists</h2>
    <div class="chips">
      {#each results.artists as artist (artist.id)}
        <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
      {/each}
    </div>
  {/if}

  {#if results.albums.length}
    <h2>Albums</h2>
    <div class="albums">
      {#each results.albums as album (album.id)}
        <a class="album" href="#/album/{album.id}">
          <Art hash={album.art_hash} size={120} round={10} />
          <span class="ellipsis">{album.title}</span>
          <span class="muted small ellipsis">{album.artist}</span>
        </a>
      {/each}
    </div>
  {/if}

  {#if results.tracks.length}
    <h2>Songs</h2>
    {#each results.tracks as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
  {#if !describedFirst}{@render describedSection()}{/if}
{:else if !query.trim()}
  <Browse />
{/if}

<style>
  .thinking {
    margin: 12px 0;
  }

  .tag {
    margin-left: 6px;
    padding: 1px 6px;
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 0.6875rem;
    font-weight: 500;
    color: var(--muted);
    vertical-align: 2px;
  }

  .described-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }

  .described-head h2 {
    margin-bottom: 2px;
  }

  .described-head p {
    margin: 0 0 8px;
  }

  .small-button {
    flex: none;
    min-height: 36px;
    margin-top: 16px;
    padding: 0 12px;
    font-size: 0.8125rem;
  }

  .search-box {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 -16px;
    padding: 12px 16px;
    background: var(--bg);
    color: var(--muted);
  }

  .field {
    position: relative;
    flex: 1;
    display: flex;
  }

  .search-box input {
    flex: 1;
    padding-right: 44px;
    font-size: 1.0625rem;
  }

  /* Our own clear button instead of the browser's, which not every browser
     shows. */
  .search-box input::-webkit-search-cancel-button {
    appearance: none;
  }

  .clear {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 44px;
    min-height: 0;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .clear:hover {
    color: var(--text);
  }

  .albums {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 120px;
    gap: 14px;
    overflow-x: auto;
    margin: 0 -16px;
    padding: 0 16px 6px;
    scrollbar-width: none;
  }

  .album {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }
</style>
