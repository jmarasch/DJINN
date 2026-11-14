<script lang="ts">
  // The library editor's workspace: the folders as they are on disk (left),
  // the songs of the chosen folder in a grid (middle), and the album's
  // cover, the changes waiting for approval and the history (right). The
  // toggle at the top says what is being edited: the library (the tags in
  // the files, only in folders allowed) or the database (DJINN's own copy,
  // the files left alone). Nothing changes until Approve: edited cells are
  // marked, and the list of changes shows what will happen.
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import Art from './Art.svelte'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditorBatch, EditorChange, EditorField, EditorNode, EditorRenamed, EditorSong, EditTarget } from './types'

  let { startFolder = '' }: { startFolder?: string } = $props()

  let target = $state<EditTarget>('database')
  let roots = $state<EditorNode[]>([])
  let children = $state<Record<string, EditorNode[]>>({})
  let open = $state<Record<string, boolean>>({})
  let selected = $state('')
  let songs = $state<EditorSong[]>([])
  let total = $state(0)
  let editable = $state(false)
  let locked = $state(false)
  let loading = $state(false)
  let busy = $state(false)
  let chosen = $state<EditorSong | null>(null)

  /** Changes waiting for approval, by song and field ('' clears the field). */
  let pending = $state<Record<number, Partial<Record<EditorField, string>>>>({})
  /** The cell being typed in. */
  let editing = $state<{ id: number; field: EditorField } | null>(null)
  let draft = $state('')
  /** Rows picked for the tools below the grid. */
  let picked = $state<Record<number, boolean>>({})
  let lastPicked = $state<number | null>(null)
  let fillField = $state<EditorField>('album_artist')
  let fillValue = $state('')
  let pattern = $state('{track} {title}')
  let renames = $state<EditorRenamed[] | null>(null)
  let history = $state<EditorBatch[]>([])
  let coverInput = $state<HTMLInputElement | null>(null)

  const FIELDS: { key: EditorField; label: string; narrow?: boolean }[] = [
    { key: 'disc_no', label: 'Disc', narrow: true },
    { key: 'track_no', label: '#', narrow: true },
    { key: 'title', label: 'Title' },
    { key: 'artist', label: 'Artist' },
    { key: 'album', label: 'Album' },
    { key: 'album_artist', label: 'Album artist' },
    { key: 'year', label: 'Year', narrow: true },
    { key: 'genre', label: 'Genre' },
  ]
  const LABEL = Object.fromEntries(FIELDS.map((f) => [f.key, f.label])) as Record<EditorField, string>
  LABEL.track_no = 'Track'

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

  const changeCount = $derived(Object.values(pending).reduce((sum, f) => sum + Object.keys(f).length, 0))

  function leaveChanges(): boolean {
    return changeCount === 0 || confirm(`Throw away ${changeCount} changes that weren't approved?`)
  }

  async function choose(node: EditorNode) {
    if (!leaveChanges()) return
    pending = {}
    selected = node.path
    chosen = null
    picked = {}
    renames = null
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
      if (!chosen || !songs.some((s) => s.id === chosen?.id)) chosen = songs[0] ?? null
      else chosen = songs.find((s) => s.id === chosen?.id) ?? null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the songs', true)
    } finally {
      loading = false
    }
  }

  async function loadHistory() {
    try {
      history = await api.editorHistory()
    } catch {
      history = []
    }
  }

  function setTarget(to: EditTarget) {
    if (to === target || !leaveChanges()) return
    pending = {}
    renames = null
    target = to
    void loadSongs(0)
    if (to === 'library') void loadHistory()
  }

  async function unlock() {
    if (await askToUnlock()) await loadRoots()
  }

  onMount(async () => {
    await loadRoots()
    if (startFolder) {
      selected = startFolder
      await loadSongs(0)
    }
  })

  /** A field as it stands now: the file's tag when editing the library, DJINN's otherwise. */
  function original(song: EditorSong, field: EditorField): string {
    const value = target === 'library' ? (song.file ? song.file[field] : undefined) : song[field]
    return value === null || value === undefined ? '' : String(value)
  }

  function shown(song: EditorSong, field: EditorField): string {
    return pending[song.id]?.[field] ?? original(song, field)
  }

  function isPending(song: EditorSong, field: EditorField) {
    return pending[song.id]?.[field] !== undefined
  }

  function setPending(song: EditorSong, field: EditorField, value: string) {
    const clean = value.trim().replace(/\s+/g, ' ')
    const fields = { ...(pending[song.id] ?? {}) }
    if (clean === original(song, field)) delete fields[field]
    else fields[field] = clean
    const next = { ...pending }
    if (Object.keys(fields).length) next[song.id] = fields
    else delete next[song.id]
    pending = next
  }

  const canEdit = $derived(target === 'database' || editable)

  function startEdit(song: EditorSong, field: EditorField) {
    if (!canEdit || (target === 'library' && !song.file)) return
    editing = { id: song.id, field }
    draft = shown(song, field)
  }

  function finishEdit(song: EditorSong, keep: boolean) {
    if (keep && editing) setPending(song, editing.field, draft)
    editing = null
  }

  function onKey(e: KeyboardEvent, song: EditorSong) {
    if (e.key === 'Enter') finishEdit(song, true)
    if (e.key === 'Escape') finishEdit(song, false)
  }

  function focusSelect(node: HTMLInputElement) {
    node.focus()
    node.select()
  }

  function pick(song: EditorSong, index: number, e: MouseEvent) {
    const on = !picked[song.id]
    const next = { ...picked }
    if (e.shiftKey && lastPicked !== null) {
      const [a, b] = [Math.min(lastPicked, index), Math.max(lastPicked, index)]
      for (const s of songs.slice(a, b + 1)) next[s.id] = true
    } else {
      next[song.id] = on
    }
    picked = next
    lastPicked = index
  }

  const pickedSongs = $derived(songs.filter((s) => picked[s.id]))
  const allPicked = $derived(songs.length > 0 && pickedSongs.length === songs.length)
  /** The songs the tools work on: those picked, else every song shown. */
  const toolSongs = $derived(pickedSongs.length ? pickedSongs : songs)

  function pickAll(on: boolean) {
    picked = on ? Object.fromEntries(songs.map((s) => [s.id, true])) : {}
  }

  function fill() {
    for (const song of toolSongs) setPending(song, fillField, fillValue)
  }

  /** Track numbers 1, 2, 3... in the grid's order, per disc. */
  function numberInOrder() {
    const next: Record<string, number> = {}
    for (const song of toolSongs) {
      const disc = shown(song, 'disc_no') || '1'
      next[disc] = (next[disc] ?? 0) + 1
      setPending(song, 'track_no', String(next[disc]))
    }
  }

  const JUNK =
    /\s*[([](official\s+(music\s+)?video|official\s+audio|official\s+lyric\s+video|lyric\s+video|lyrics?|audio|video|hd|hq|4k|visuali[sz]er)[)\]]/gi

  /** A title without a leading track number or a video site's leftovers. */
  function cleanTitle(title: string): string {
    return title
      .replace(JUNK, '')
      .replace(/^\s*\d{1,3}\s*[-._)]\s+(?=\S)/, '')
      .replace(/\s+/g, ' ')
      .trim()
  }

  function cleanTitles() {
    let changed = 0
    for (const song of toolSongs) {
      const now = shown(song, 'title')
      const clean = cleanTitle(now)
      if (clean && clean !== now) {
        setPending(song, 'title', clean)
        changed++
      }
    }
    toast(changed ? `${changed} titles cleaned up; check them under Changes` : 'Nothing to clean up')
  }

  /** The changes as the server takes them. */
  function changeList(): EditorChange[] {
    const out: EditorChange[] = []
    for (const [id, fields] of Object.entries(pending)) {
      for (const [field, value] of Object.entries(fields)) {
        out.push({ id: Number(id), field: field as EditorField, value: value === '' ? null : (value ?? null) })
      }
    }
    return out
  }

  const review = $derived(
    Object.entries(pending).flatMap(([id, fields]) => {
      const song = songs.find((s) => s.id === Number(id))
      return Object.entries(fields).map(([field, value]) => ({
        id: Number(id),
        song: song?.title ?? `Song ${id}`,
        field: field as EditorField,
        from: song ? original(song, field as EditorField) : '',
        to: value ?? '',
      }))
    }),
  )

  function undoOne(id: number, field: EditorField) {
    const fields = { ...(pending[id] ?? {}) }
    delete fields[field]
    const next = { ...pending }
    if (Object.keys(fields).length) next[id] = fields
    else delete next[id]
    pending = next
  }

  async function approve() {
    const changes = changeList()
    if (!changes.length) return
    const songCount = Object.keys(pending).length
    if (
      target === 'library' &&
      !confirm(`Write ${changes.length} changes into ${songCount} music files? This can be undone from the history.`)
    )
      return
    busy = true
    try {
      const done = await unlocked(() => api.editorApply(target, changes))
      pending = {}
      report(done.songs, done.failed, target === 'library' ? 'files written' : 'songs changed')
      await loadSongs(0)
      if (target === 'library') await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not approve', true)
    } finally {
      busy = false
    }
  }

  function report(count: number, failed: { id: number; error: string }[], what: string) {
    if (failed.length) {
      const first = failed[0]
      const name = songs.find((s) => s.id === first.id)?.title ?? `song ${first.id}`
      toast(`${n(count)} ${what}; ${failed.length} couldn't be: ${name}: ${first.error}`, true)
    } else {
      toast(`${n(count)} ${what}`)
    }
  }

  async function undo(batch: EditorBatch) {
    if (!confirm(`Put back what approval ${batch.batch} changed in ${batch.files} files?`)) return
    busy = true
    try {
      const done = await unlocked(() => api.editorUndo(batch.batch))
      report(done.songs, done.failed, 'files put back')
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not undo', true)
    } finally {
      busy = false
    }
  }

  /** The songs a cover goes to: those picked, else the chosen song's album here. */
  const coverSongs = $derived(
    pickedSongs.length
      ? pickedSongs
      : chosen
        ? songs.filter((s) => (chosen?.album_id != null ? s.album_id === chosen.album_id : s.id === chosen?.id))
        : [],
  )

  async function setCover(how: 'set' | 'remove' | 'reset', file: File | null = null) {
    if (!coverSongs.length) return
    const where = target === 'library' ? 'into the files of' : 'for'
    const words = how === 'set' ? 'Use this cover' : how === 'remove' ? 'Take the cover out' : "Go back to the files' own cover"
    if (!confirm(`${words} ${where} ${coverSongs.length} songs?`)) return
    busy = true
    try {
      const done = await unlocked(() => api.editorCover(target, coverSongs.map((s) => s.id), file, how))
      report(done.songs, done.failed, target === 'library' ? 'files written' : 'songs changed')
      await loadSongs(0)
      if (target === 'library') await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change the cover', true)
    } finally {
      busy = false
      if (coverInput) coverInput.value = ''
    }
  }

  async function previewRename() {
    if (changeCount && !confirm('Rename from the tags as they are now? Changes not approved yet are left out.')) return
    try {
      renames = (await unlocked(() => api.editorRename(toolSongs.map((s) => s.id), pattern, true))).files
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not work out the names', true)
    }
  }

  const renamable = $derived(renames?.filter((r) => !r.problem) ?? [])

  async function rename() {
    if (!renamable.length) return
    if (!confirm(`Rename ${renamable.length} files on disk? This can be undone from the history.`)) return
    busy = true
    try {
      const done = await unlocked(() => api.editorRename(renamable.map((r) => r.id), pattern, false))
      const moved = done.files.filter((f) => !f.problem).length
      toast(`${n(moved)} files renamed`)
      renames = null
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not rename', true)
    } finally {
      busy = false
    }
  }

  const fileName = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)
  const KINDS: Record<string, string> = { tags: 'tags', cover: 'cover', rename: 'names' }
  const kindName = (kinds: string) =>
    kinds.startsWith('undo') ? 'undo' : kinds.split(',').map((k) => KINDS[k] ?? k).join(', ')
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
          Editing the tags in the music files themselves. Only folders you allowed can be changed, and every approval
          can be undone from the history.
        {:else}
          Editing DJINN's own copy of the tags. The files are left alone. Clearing a cell goes back to the file's value.
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
            {n(songs.length)} of {n(total)} songs{pickedSongs.length ? ` · ${n(pickedSongs.length)} picked` : ''}
            {#if target === 'library' && !editable}· <strong>view only</strong>: changes aren't allowed in this folder
              (allow it under Compare and folders){:else}· click a cell to change it{/if}
          </p>
          <div class="scroll">
            <table>
              <thead>
                <tr>
                  <th class="pick">
                    <input type="checkbox" aria-label="Pick every song" checked={allPicked} onchange={(e) => pickAll(e.currentTarget.checked)} />
                  </th>
                  {#each FIELDS as f (f.key)}<th>{f.label}</th>{/each}
                  <th>File</th>
                </tr>
              </thead>
              <tbody>
                {#each songs as song, i (song.id)}
                  <tr class:chosen={chosen?.id === song.id} class:unreadable={target === 'library' && !song.file}>
                    <td class="pick">
                      <input
                        type="checkbox"
                        aria-label="Pick {song.title}"
                        checked={!!picked[song.id]}
                        onclick={(e) => pick(song, i, e)}
                      />
                    </td>
                    {#each FIELDS as f (f.key)}
                      <td
                        class:narrow={f.narrow}
                        class:pending={isPending(song, f.key)}
                        title={isPending(song, f.key) ? `Was: ${original(song, f.key) || '(empty)'}` : undefined}
                        onclick={() => ((chosen = song), startEdit(song, f.key))}
                      >
                        {#if editing?.id === song.id && editing.field === f.key}
                          <input
                            class="cell"
                            bind:value={draft}
                            use:focusSelect
                            onkeydown={(e) => onKey(e, song)}
                            onblur={() => finishEdit(song, true)}
                            inputmode={f.narrow ? 'numeric' : undefined}
                          />
                        {:else}
                          {shown(song, f.key)}
                        {/if}
                      </td>
                    {/each}
                    <td class="mono muted" title={song.path}>{fileName(song.path)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if songs.length < total}
            <button class="small-button pad" disabled={loading} onclick={() => loadSongs(songs.length)}>Show more</button>
          {/if}

          {#if canEdit && songs.length}
            <div class="tools">
              <div class="tool">
                <span class="small muted">{pickedSongs.length ? `For the ${pickedSongs.length} picked songs` : 'For every song shown'}:</span>
              </div>
              <div class="tool">
                <select bind:value={fillField} aria-label="Field to set">
                  {#each FIELDS as f (f.key)}<option value={f.key}>{LABEL[f.key]}</option>{/each}
                </select>
                <input bind:value={fillValue} placeholder="Value (empty clears it)" aria-label="Value" />
                <button class="small-button" onclick={fill}>Set</button>
              </div>
              <div class="tool">
                <button class="small-button" onclick={numberInOrder} title="Track numbers 1, 2, 3... in the grid's order, per disc">
                  Number in order
                </button>
                <button
                  class="small-button"
                  onclick={cleanTitles}
                  title="Takes off a leading track number and leftovers like (Official Video) or [HD]"
                >
                  Clean up titles
                </button>
              </div>
              {#if target === 'library'}
                <div class="tool">
                  <input bind:value={pattern} aria-label="File name pattern" class="pattern" />
                  <button class="small-button" onclick={previewRename}>Rename files...</button>
                  <span class="small muted">{'{disc} {track} {title} {artist} {album} {album_artist} {year}'}</span>
                </div>
              {/if}
            </div>
          {/if}

          {#if renames}
            <div class="renames">
              <div><strong>New file names</strong> <span class="small muted">({renamable.length} to rename)</span></div>
              <ul>
                {#each renames as r (r.id)}
                  <li class="small" class:bad={!!r.problem}>
                    <span class="mono">{fileName(r.from)}</span> → <span class="mono">{fileName(r.to)}</span>
                    {#if r.problem}<em>({r.problem})</em>{/if}
                  </li>
                {/each}
              </ul>
              <button class="primary" disabled={busy || !renamable.length} onclick={rename}>Rename {renamable.length} files</button>
              <button onclick={() => (renames = null)}>Cancel</button>
            </div>
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
          <div class="small muted">
            Cover {coverSongs.length ? `for ${coverSongs.length} ${pickedSongs.length ? 'picked songs' : 'songs of this album'}` : ''}
          </div>
          <input
            type="file"
            accept="image/jpeg,image/png"
            hidden
            bind:this={coverInput}
            onchange={(e) => {
              const file = e.currentTarget.files?.[0]
              if (file) void setCover('set', file)
            }}
          />
          <button class="small-button" disabled={!canEdit || busy || !coverSongs.length} onclick={() => coverInput?.click()}>
            Replace cover
          </button>
          <button class="small-button" disabled={!canEdit || busy || !coverSongs.length} onclick={() => setCover('remove')}>
            Remove cover
          </button>
          {#if target === 'database'}
            <button class="small-button" disabled={busy || !coverSongs.length} onclick={() => setCover('reset')}>
              Use the files' cover
            </button>
          {/if}
        </div>
        <div class="box review">
          <div><strong>Preview and approve changes</strong></div>
          {#if review.length}
            <p class="small muted">
              {n(review.length)} changes to {n(Object.keys(pending).length)} songs, going
              {target === 'library' ? 'into the files' : "into DJINN's database"}:
            </p>
            <ul class="changes">
              {#each review.slice(0, 200) as c (`${c.id}-${c.field}`)}
                <li class="small">
                  <span class="muted">{c.song} · {LABEL[c.field]}:</span>
                  <s>{c.from || '(empty)'}</s> → <b>{c.to || '(empty)'}</b>
                  <button class="link" aria-label="Leave this one" onclick={() => undoOne(c.id, c.field)}>×</button>
                </li>
              {/each}
              {#if review.length > 200}<li class="small muted">and {n(review.length - 200)} more</li>{/if}
            </ul>
            <button class="primary" disabled={busy || !canEdit} onclick={approve}>Approve {n(review.length)} changes</button>
            <button class="small-button" disabled={busy} onclick={() => (pending = {})}>Throw them away</button>
          {:else}
            <p class="small muted">No changes yet. Click a cell to change it, or use the tools under the grid.</p>
            <button class="primary" disabled>Approve changes</button>
          {/if}
        </div>
        {#if target === 'library'}
          <div class="box">
            <div class="small muted">Changes made to files</div>
            {#if history.length}
              <ul class="history">
                {#each history.slice(0, 12) as b (b.batch)}
                  <li class="small">
                    {new Date(b.made_at).toLocaleString()} · {kindName(b.kinds)} · {n(b.files)} files{b.made_by
                      ? ` · ${b.made_by}`
                      : ''}
                    {#if b.undone_at}
                      <span class="muted">(undone)</span>
                    {:else if !b.kinds.startsWith('undo')}
                      <button class="link" disabled={busy} onclick={() => undo(b)}>Undo</button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="small muted">None yet.</p>
            {/if}
          </div>
        {/if}
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
    grid-template-columns: 280px minmax(0, 1fr) 260px;
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
    padding-left: 14px;
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
    max-height: 60vh;
  }

  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 0.88rem;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
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
    cursor: text;
  }

  td.narrow {
    max-width: 7ch;
  }

  td.pick,
  th.pick {
    width: 28px;
    cursor: default;
  }

  td.pending {
    background: color-mix(in srgb, var(--target) 25%, transparent);
    outline: 1px solid var(--target);
    outline-offset: -1px;
  }

  input.cell {
    width: 100%;
    min-width: 8ch;
    padding: 1px 4px;
    font: inherit;
  }

  tr.chosen td {
    background: var(--card-bg, transparent);
  }

  tr.chosen td.pending {
    background: color-mix(in srgb, var(--target) 25%, transparent);
  }

  tr.unreadable td {
    opacity: 0.5;
  }

  .pad {
    padding: 6px 10px;
    margin: 0;
  }

  .tools {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border-top: 1px solid var(--line);
  }

  .tool {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .pattern {
    min-width: 16ch;
  }

  .renames {
    padding: 8px 10px;
    border-top: 1px solid var(--target);
  }

  .renames ul,
  .changes,
  .history {
    list-style: none;
    margin: 6px 0;
    padding: 0;
    max-height: 30vh;
    overflow: auto;
  }

  .changes li,
  .history li {
    padding: 2px 0;
    border-bottom: 1px solid var(--line);
    overflow-wrap: anywhere;
  }

  .bad {
    color: var(--bad, #c0392b);
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }

  .box {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }

  .box ul {
    align-self: stretch;
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
