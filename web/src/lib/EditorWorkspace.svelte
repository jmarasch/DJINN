<script lang="ts">
  // The library editor's workspace: the folders as they are on disk (left),
  // the songs of the chosen folder in a grid (middle), and the album's
  // cover with the preview and approval of changes (right). The toggle at
  // the top says what is being edited: the library (the tags in the files)
  // or the database (DJINN's own copy). This first piece shows; editing,
  // preview and approval come next.
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import Art from './Art.svelte'
  import { askToUnlock } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditorNode, EditorSong } from './types'

  let target = $state<'database' | 'library'>('database')
  let roots = $state<EditorNode[]>([])
  let children = $state<Record<string, EditorNode[]>>({})
  let open = $state<Record<string, boolean>>({})
  let selected = $state('')
  let songs = $state<EditorSong[]>([])
  let total = $state(0)
  let editable = $state(false)
  let locked = $state(false)
  let loading = $state(false)
  let chosen = $state<EditorSong | null>(null)

  const n = (x: number) => x.toLocaleString()

  async function loadRoots() {
    try {
      roots = (await api.editorTree('')).folders
      locked = false
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) locked = true
      else toast(e instanceof Error ? e.message : 'Could not load', true)
    }
  }

  async function toggle(node: EditorNode) {
    if (open[node.path]) {
      open = { ...open, [node.path]: false }
      return
    }
    if (!children[node.path]) {
      try {
        children = { ...children, [node.path]: (await api.editorTree(node.path)).folders }
      } catch (e) {
        toast(e instanceof Error ? e.message : 'Could not open it', true)
        return
      }
    }
    open = { ...open, [node.path]: true }
  }

  async function choose(node: EditorNode) {
    selected = node.path
    chosen = null
    await loadSongs(0)
  }

  async function loadSongs(offset: number) {
    if (!selected) return
    loading = true
    try {
      const found = await api.editorSongs(selected, offset, target === 'library')
      songs = offset === 0 ? found.songs : [...songs, ...found.songs]
      total = found.total
      editable = found.editable
      if (!chosen) chosen = songs[0] ?? null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the songs', true)
    } finally {
      loading = false
    }
  }

  function setTarget(to: 'database' | 'library') {
    target = to
    void loadSongs(0)
  }

  async function unlock() {
    if (await askToUnlock()) await loadRoots()
  }

  onMount(() => void loadRoots())

  /** The value a cell shows: the file's tag when editing the library, DJINN's otherwise. */
  function cell(song: EditorSong, field: 'title' | 'artist' | 'album' | 'album_artist' | 'track_no' | 'disc_no' | 'year' | 'genre') {
    const value = target === 'library' ? (song.file ? song.file[field] : undefined) : song[field]
    return value === null || value === undefined ? '' : String(value)
  }

  const fileName = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)
</script>

{#snippet branch(nodes: EditorNode[], depth: number)}
  <ul class="tree" style:--depth={depth}>
    {#each nodes as node (node.path)}
      <li>
        <div class="node" class:picked={selected === node.path}>
          <button class="twisty" aria-label={open[node.path] ? 'Close' : 'Open'} onclick={() => toggle(node)}>
            {open[node.path] ? '−' : '+'}
          </button>
          <button class="label" onclick={() => choose(node)} title={node.path}>
            {node.name}
            <span class="count">{n(node.songs)}</span>
            {#if node.editable === true}<span class="badge on">Editable</span>{:else if node.editable === false}<span class="badge">View only</span>{/if}
          </button>
        </div>
        {#if open[node.path] && children[node.path]}
          {@render branch(children[node.path], depth + 1)}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#if locked}
  <section class="card">
    <p class="small">The editor is locked. <button class="link" onclick={unlock}>Unlock</button></p>
  </section>
{:else}
  <div class="editor" class:library={target === 'library'}>
    <div class="top">
      <div class="toggle" role="group" aria-label="What you are editing">
        <button class:on={target === 'library'} onclick={() => setTarget('library')}>Library</button>
        <button class:on={target === 'database'} onclick={() => setTarget('database')}>Database</button>
      </div>
      <p class="small muted">
        {#if target === 'library'}
          Editing the tags in the music files themselves. Only folders you allowed can be changed.
        {:else}
          Editing DJINN's own copy of the tags. The files are left alone.
        {/if}
      </p>
    </div>

    <div class="panes">
      <aside class="folders">
        {#if roots.length}
          {@render branch(roots, 0)}
        {:else}
          <p class="small muted">No library folders.</p>
        {/if}
      </aside>

      <section class="grid">
        {#if !selected}
          <p class="small muted pad">Choose a folder on the left to see its songs.</p>
        {:else}
          <p class="small muted pad">
            {n(songs.length)} of {n(total)} songs
            {#if target === 'library' && !editable}· <strong>view only</strong>: changes aren't allowed in this folder{/if}
          </p>
          <div class="scroll">
            <table>
              <thead>
                <tr>
                  <th>Disc</th><th>#</th><th>Title</th><th>Artist</th><th>Album</th><th>Album artist</th><th>Year</th><th>Genre</th><th>File</th>
                </tr>
              </thead>
              <tbody>
                {#each songs as song (song.id)}
                  <tr class:chosen={chosen?.id === song.id} class:unreadable={target === 'library' && !song.file} onclick={() => (chosen = song)}>
                    <td>{cell(song, 'disc_no')}</td>
                    <td>{cell(song, 'track_no')}</td>
                    <td>{cell(song, 'title')}</td>
                    <td>{cell(song, 'artist')}</td>
                    <td>{cell(song, 'album')}</td>
                    <td>{cell(song, 'album_artist')}</td>
                    <td>{cell(song, 'year')}</td>
                    <td>{cell(song, 'genre')}</td>
                    <td class="mono muted" title={song.path}>{fileName(song.path)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if songs.length < total}
            <button class="small-button pad" disabled={loading} onclick={() => loadSongs(songs.length)}>Show more</button>
          {/if}
        {/if}
      </section>

      <aside class="side">
        <div class="box art">
          <div class="small muted">Album art</div>
          {#if chosen}
            <Art hash={chosen.art_hash} size={200} round={8} />
            <div class="small">{chosen.album ?? ''}</div>
          {:else}
            <p class="small muted">Choose a song to see its album's cover.</p>
          {/if}
        </div>
        <div class="box">
          <div class="small muted">Cover</div>
          <button class="small-button" disabled title="Coming next">Replace cover</button>
          <button class="small-button" disabled title="Coming next">Remove cover</button>
        </div>
        <div class="box review">
          <div><strong>Preview and approve changes</strong></div>
          <p class="small muted">No changes yet. Editing cells comes next; each change will be listed here to approve.</p>
          <button class="primary" disabled>Approve changes</button>
        </div>
      </aside>
    </div>
  </div>
{/if}

<style>
  .editor {
    --target: var(--accent);
    border: 2px solid var(--target);
    border-radius: 12px;
    padding: 12px;
    box-shadow: 0 0 14px var(--glow);
  }

  .editor.library {
    --target: #e8a317;
    box-shadow: 0 0 14px rgb(232 163 23 / 0.4);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    margin-bottom: 10px;
  }

  .toggle {
    display: inline-flex;
    border: 1px solid var(--target);
    border-radius: 999px;
    overflow: hidden;
  }

  .toggle button {
    border: 0;
    border-radius: 0;
    padding: 8px 18px;
    background: none;
    box-shadow: none;
  }

  .toggle button.on {
    background: var(--target);
    color: #000;
    font-weight: 700;
  }

  .panes {
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr) 240px;
    gap: 12px;
    min-height: 60vh;
  }

  .folders,
  .grid,
  .box {
    border: 1px solid var(--line);
    border-radius: 8px;
  }

  .folders {
    overflow: auto;
    max-height: 72vh;
    padding: 6px;
  }

  .tree {
    list-style: none;
    margin: 0;
    padding-left: calc(var(--depth, 0) * 0px + 14px);
  }

  .folders > .tree {
    padding-left: 0;
  }

  .node {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .node.picked .label {
    background: var(--card-bg, transparent);
    outline: 1px solid var(--target);
  }

  .twisty {
    width: 24px;
    padding: 0;
    border: 0;
    background: none;
    box-shadow: none;
  }

  .label {
    flex: 1;
    text-align: left;
    border: 0;
    background: none;
    box-shadow: none;
    padding: 3px 6px;
    border-radius: 6px;
  }

  .count {
    color: var(--muted);
    font-size: 0.8em;
    margin-left: 6px;
  }

  .badge.on {
    background: var(--accent);
    color: #000;
  }

  .grid {
    min-width: 0;
  }

  .scroll {
    overflow: auto;
    max-height: 66vh;
  }

  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 0.88rem;
  }

  th {
    position: sticky;
    top: 0;
    background: var(--page-bg, #000);
    text-align: left;
    padding: 4px 8px;
    border-bottom: 1px solid var(--target);
  }

  td {
    padding: 3px 8px;
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
    max-width: 22ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  tr.chosen td {
    background: var(--card-bg, transparent);
  }

  tr.unreadable td {
    opacity: 0.5;
  }

  .pad {
    padding: 6px 10px;
    margin: 0;
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .box {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }

  .review {
    border-color: var(--target);
    box-shadow: 0 0 10px var(--glow);
  }

  @media (max-width: 1100px) {
    .panes {
      grid-template-columns: 1fr;
    }
  }
</style>
