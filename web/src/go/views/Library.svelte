<script lang="ts">
  // Your library: liked songs, albums and artists, playlists, history.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import ScrollRow from '../../lib/ScrollRow.svelte'
  import { app, toast } from '../../lib/state.svelte'
  import GenreCloud from '../../lib/GenreCloud.svelte'
  import GenreTags from '../../lib/GenreTags.svelte'
  import Icon from '../../lib/Icon.svelte'
  import ImportPlaylist from '../../lib/ImportPlaylist.svelte'
  import type { GenreMap, GoPlaylist, Liked, Played } from '../../lib/types'
  import { playList, tapSong } from '../player.svelte'
  import SongList from '../SongList.svelte'
  import LikeButton from '../LikeButton.svelte'
  import SongRow from '../SongRow.svelte'
  import YourYear from './YourYear.svelte'

  let { section = 'liked' }: { section?: 'liked' | 'playlists' | 'genres' | 'history' | 'year' } = $props()

  let liked = $state<Liked | null>(null)
  let lists = $state<GoPlaylist[]>([])
  let history = $state<Played[]>([])
  let more = $state(true)
  let name = $state('')
  let genres = $state<GenreMap | null>(null)
  let genreView = $state<'tags' | 'cloud'>(storedGenreView())

  function storedGenreView() {
    try {
      return localStorage.getItem('djinn-genre-view') === 'cloud' ? 'cloud' : 'tags'
    } catch {
      return 'tags'
    }
  }

  function showGenres(view: 'tags' | 'cloud') {
    genreView = view
    try {
      localStorage.setItem('djinn-genre-view', view)
    } catch {
      // Only a convenience.
    }
  }

  $effect(() => {
    load(section).catch((e) => toast(e.message, true))
  })

  async function load(which: string) {
    if (which === 'liked') liked = await api.goLiked()
    else if (which === 'playlists') lists = await api.goPlaylists()
    else if (which === 'genres') genres = await api.genres()
    else if (which === 'year') return
    else {
      history = await api.goHistory(0)
      more = history.length === 100
    }
  }

  async function loadMore() {
    const next = await api.goHistory(history.length)
    history = [...history, ...next]
    more = next.length === 100
  }

  async function create(e: SubmitEvent) {
    e.preventDefault()
    try {
      const list = await api.goCreatePlaylist(name)
      name = ''
      location.hash = `#/playlist/${list.id}`
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not make it', true)
    }
  }

  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

<nav class="sections">
  <a href="#/library" class:active={section === 'liked'}>Liked</a>
  <a href="#/library/playlists" class:active={section === 'playlists'}>Playlists</a>
  <a href="#/library/genres" class:active={section === 'genres'}>Genres</a>
  <a href="#/library/history" class:active={section === 'history'}>History</a>
  <a href="#/library/year" class:active={section === 'year'}>Your year</a>
  {#if app.me?.permissions?.submit_music}<a href="#/submit">Submit music</a>{/if}
</nav>

{#if section === 'year'}
  <YourYear />
{/if}

{#if section === 'genres'}
  <div class="genre-views" role="group" aria-label="Layout">
    <button class:active={genreView === 'tags'} aria-label="List" aria-pressed={genreView === 'tags'} onclick={() => showGenres('tags')}>
      <Icon name="list" size={20} />
    </button>
    <button class:active={genreView === 'cloud'} aria-label="Cloud" aria-pressed={genreView === 'cloud'} onclick={() => showGenres('cloud')}>
      <Icon name="cloud" size={20} />
    </button>
  </div>
  {#if !genres}
    <p class="muted small">Loading genres...</p>
  {:else if !genres.genres.length}
    <p class="muted">No genres yet.</p>
  {:else if genreView === 'cloud'}
    <GenreCloud />
  {:else}
    <GenreTags map={genres} />
  {/if}
{/if}

{#if section === 'liked' && liked}
  {#if liked.artists.length}
    <h2>Artists</h2>
    <ScrollRow gap={12}>
      {#each liked.artists as artist (artist.id)}
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
  {#if liked.albums.length}
    <h2>Albums</h2>
    <ScrollRow gap={12}>
      {#each liked.albums as album (album.id)}
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
  <div class="head">
    <h2>Songs</h2>
    {#if liked.tracks.length}<button class="small-button" onclick={() => liked && playList(liked.tracks)}>Play all</button>{/if}
  </div>
  {#if liked.tracks.length}
    <SongList tracks={liked.tracks} />
  {:else}
    <p class="muted">Tap the heart on a song to keep it here.</p>
  {/if}
{:else if section === 'playlists'}
  <ImportPlaylist
    onimported={() => {
      api.goPlaylists().then((found) => (lists = found))
    }}
  />
  <form class="new" onsubmit={create}>
    <input bind:value={name} placeholder="New playlist" maxlength="100" required aria-label="New playlist name" />
    <button class="primary">Make</button>
  </form>
  {#each lists as list (list.id)}
    <a class="list" href="#/playlist/{list.id}">
      <span>{list.name}</span>
      <span class="muted small">
        {list.tracks} {list.tracks === 1 ? 'song' : 'songs'}{#if list.user_id !== app.me?.id}, by {list.owner}{:else if list.shared}, shared{/if}
      </span>
    </a>
  {:else}
    <p class="muted">No playlists yet.</p>
  {/each}
{:else if section === 'history'}
  {#each history as played, i (`${played.id}-${played.played_at}-${i}`)}
    <div class="played">
      <SongRow track={played} onplay={() => tapSong(played, () => playList(history, i))} />
      <span class="muted small at">{when(played.played_at)}</span>
    </div>
  {:else}
    <p class="muted">Songs you play show up here.</p>
  {/each}
  {#if more && history.length}<button class="more" onclick={loadMore}>Show more</button>{/if}
{/if}

<style>
  .sections {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 8px;
  }

  .sections a {
    padding: 8px 14px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text);
    text-decoration: none;
  }

  .sections a.active {
    background: var(--accent);
    color: var(--accent-text);
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 1.05rem;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
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

  .new {
    display: flex;
    gap: 8px;
    margin: 8px 0;
  }

  .new input {
    flex: 1;
    min-width: 0;
  }

  .list {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 12px 4px;
    border-bottom: 1px solid var(--line);
    color: inherit;
    text-decoration: none;
  }

  .played {
    display: flex;
    flex-direction: column;
  }

  .at {
    margin: -4px 0 4px 60px;
  }

  .more {
    width: 100%;
    margin-top: 8px;
  }

  .genre-views {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-bottom: 8px;
  }

  .genre-views button {
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: var(--surface-2);
    color: var(--muted);
  }

  .genre-views button.active {
    background: var(--surface-3);
    color: var(--text);
  }
</style>
