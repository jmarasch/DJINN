<script lang="ts">
  // The whole library: search as you type, or browse by artist, album,
  // song or genre (the same browser as the party app). Tapping a song in
  // the big lists plays it, then songs like it. Descriptions ("chill house",
  // "upbeat like KREAM") also list the songs that fit, read by the AI reader
  // when the phrase reader doesn't know every word.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import ScrollRow from '../../lib/ScrollRow.svelte'
  import type { SearchResults, Track } from '../../lib/types'
  import Browse from '../../screens/Browse.svelte'
  import { playList, playRadio, tapSong } from '../player.svelte'
  import SongList from '../SongList.svelte'
  import SongRow from '../SongRow.svelte'
  import LikeButton from '../LikeButton.svelte'
  import SongTile from '../SongTile.svelte'
  import NotInLibrary from '../../lib/NotInLibrary.svelte'
  import { toast } from '../../lib/state.svelte'

  let searchBar = $state<HTMLElement>()

  let query = $state(remembered())
  let results = $state<SearchResults | null>(null)
  let error = $state('')
  /** The AI reader is reading the query. */
  let thinking = $state(false)
  let latest = 0

  function remembered(): string {
    try {
      return sessionStorage.getItem('go-search') ?? ''
    } catch {
      return ''
    }
  }

  $effect(() => {
    const q = query.trim()
    try {
      sessionStorage.setItem('go-search', query)
    } catch {
      // Private browsing: the search just isn't remembered.
    }
    if (!q) {
      results = null
      return
    }
    const id = ++latest
    thinking = false
    const timer = setTimeout(async () => {
      try {
        const found = await api.search(q)
        if (id !== latest) return
        results = found
        error = ''
        if (!found.ask) return
        // Words the phrase reader didn't know: once typing stops, let the
        // AI reader have a go (a few seconds).
        thinking = true
        await new Promise((resolve) => setTimeout(resolve, 600))
        if (id !== latest) return
        const read = await api.ask(q)
        if (id === latest && read && results) results = { ...results, described: read }
      } catch (e) {
        if (id === latest && !results) error = e instanceof Error ? e.message : 'Search failed'
      } finally {
        if (id === latest) thinking = false
      }
    }, 250)
    return () => clearTimeout(timer)
  })

  const described = $derived(results?.described ?? null)
  /** Matches by name come first when there are any; a description with
   * nothing by that name leads with what fits. */
  const describedFirst = $derived(!!described && !results?.tracks.length && !results?.artists.length)

  async function savePlaylist() {
    if (!described) return
    const label = described.summary.join(' ')
    const name = label.charAt(0).toUpperCase() + label.slice(1)
    try {
      const list = await api.goCreatePlaylist(name, described.tracks.map((t) => t.id))
      toast(`Made the playlist ${list.name}`)
      location.hash = `/playlist/${list.id}`
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not make the playlist', true)
    }
  }

  /** A tap in the big lists: the song, then songs like it (or into the
   * queue, if that's what taps do). */
  const play = (track: Track) => tapSong(track, () => playRadio(track))
</script>

{#snippet describedSection()}
  {#if thinking}
    <p class="small muted thinking"><span class="spinner" aria-hidden="true"></span> Thinking...</p>
  {/if}
  {#if described}
    <div class="described-head">
      <h2>
        Matches <span class="muted">{described.summary.join(' · ')}</span>
        {#if described.read_by_ai}<span class="tag">read by AI</span>{/if}
      </h2>
      {#if described.tracks.length}
        <p class="small muted">
          {described.total} {described.total === 1 ? 'song fits' : 'songs fit'}{described.total > described.tracks.length
            ? `, showing ${described.tracks.length}`
            : ''}.{described.unknown.length ? ` Not sure about "${described.unknown.join(' ')}".` : ''}
        </p>
        <div class="actions">
          <button onclick={() => playList(described.tracks)}>Play all</button>
          <button onclick={savePlaylist}>Save as playlist</button>
        </div>
      {:else}
        <p class="small muted">Nothing in the library fits that.</p>
      {/if}
    </div>
    {#if described.artists.length}
      <div class="chips">
        {#each described.artists as artist (artist.id)}
          <a class="chip" href="#/artist/{artist.id}">{artist.name}</a>
        {/each}
      </div>
    {/if}
    <SongList tracks={described.tracks} />
  {/if}
{/snippet}

<div class="search-bar" bind:this={searchBar}>
  <input
    class="search"
    type="search"
    placeholder="Search, or describe: 80s rock, chill house..."
    bind:value={query}
    aria-label="Search"
    autocomplete="off"
    autocapitalize="none"
    enterkeyhint="search"
  />
</div>
{#if error}<p class="error">{error}</p>{/if}

{#if !query.trim()}
  <Browse progress={false} stuckBelow={searchBar}>
    {#snippet song(track)}
      <SongRow {track} onplay={() => play(track)} />
    {/snippet}
    {#snippet songTile(track)}
      <SongTile {track} onplay={() => play(track)} />
    {/snippet}
    {#snippet like(kind, id, name)}
      <LikeButton {kind} {id} {name} />
    {/snippet}
  </Browse>
{:else if results}
  {#if describedFirst}{@render describedSection()}{/if}
  {#if results.artists.length}
    <h2>Artists</h2>
    <ScrollRow gap={12}>
      {#each results.artists as artist (artist.id)}
        <div class="tile-wrap">
          <a class="tile" href="#/artist/{artist.id}">
            <Art hash={artist.art_hash} size={88} round={999} letter={artist.name[0]} />
            <span class="small">{artist.name}</span>
          </a>
          <div class="corner"><LikeButton kind="artist" id={artist.id} name={artist.name} size={18} /></div>
        </div>
      {/each}
    </ScrollRow>
  {/if}
  {#if results.albums.length}
    <h2>Albums</h2>
    <ScrollRow gap={12}>
      {#each results.albums as album (album.id)}
        <div class="tile-wrap">
          <a class="tile" href="#/album/{album.id}">
            <Art hash={album.art_hash} size={88} />
            <span class="small">{album.title}</span>
            <span class="muted small">{album.artist}</span>
          </a>
          <div class="corner"><LikeButton kind="album" id={album.id} name={album.title} size={18} /></div>
        </div>
      {/each}
    </ScrollRow>
  {/if}
  {#if results.tracks.length}
    <h2>Songs</h2>
    <SongList tracks={results.tracks} />
  {:else if !described?.tracks.length && !results.lyrics?.length && !thinking}
    <h2>Songs</h2>
    <p class="muted">No songs match.</p>
  {/if}
  {#if results.lyrics?.length}
    <h2>Songs with these words</h2>
    {#each results.lyrics as hit (hit.track.id)}
      <SongRow
        track={hit.track}
        onplay={() => tapSong(hit.track, () => playList(results?.lyrics?.map((h) => h.track) ?? [hit.track], results?.lyrics?.indexOf(hit) ?? 0))}
      />
      <p class="quote">“{hit.line}”</p>
    {/each}
  {/if}
  {#if !describedFirst}{@render describedSection()}{/if}
  <NotInLibrary {query} />
{/if}

<style>
  /* The lyric line a search matched, under its song. */
  .quote {
    margin: -6px 0 10px 56px;
    color: var(--muted);
    font-size: 0.875rem;
    font-style: italic;
  }

  .search-bar {
    position: sticky;
    top: var(--nav-height, 60px);
    z-index: 5;
    padding: 4px 0 8px;
    background: var(--bg);
  }

  .search {
    width: 100%;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .thinking {
    display: flex;
    align-items: center;
    gap: 8px;
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

  .described-head h2 {
    margin-bottom: 2px;
  }

  .described-head p {
    margin: 0 0 8px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
  }

  .chips {
    margin-bottom: 8px;
  }

  .tile {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 4px;
    width: 88px;
    color: inherit;
    text-decoration: none;
  }

  .tile-wrap {
    position: relative;
    flex: none;
  }

  /* The heart sits on the picture's corner. */
  .corner {
    position: absolute;
    top: 0;
    right: 0;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.45);
  }

  .corner :global(button:not(.liked)) {
    color: #fff;
  }

  .tile span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
