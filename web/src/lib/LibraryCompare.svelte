<script lang="ts">
  // Admin > Library editor > Compare, and which folders can be changed.
  // Compares what the database holds with what each file's own tags say,
  // field by field, and why they differ.
  // It only reads: nothing in the music folders or the database changes.
  // The editor sits behind the protected areas' lock.
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { CompareShown, CompareWhy, EditableFolder } from './types'

  const whyLabel: Record<CompareWhy, string> = {
    correction: 'Corrected in DJINN',
    identified: 'Filled in by identification',
    other: 'Other',
  }
  const whyHelp: Record<CompareWhy, string> = {
    correction: 'You changed it with Edit tags or Edit album. The file still says the old value.',
    identified: "The file has nothing there and an applied identification filled it in.",
    other: 'The file changed since DJINN read it, or the two readings differ. A library check re-reads changed files.',
  }
  const fieldLabel: Record<string, string> = {
    title: 'Title',
    artist: 'Artist',
    album: 'Album',
    album_artist: 'Album artist',
    track_no: 'Track number',
    disc_no: 'Disc number',
    year: 'Year',
    genre: 'Genre',
  }

  let shown = $state<CompareShown | null>(null)
  let folders = $state<EditableFolder[]>([])
  let folder = $state('')
  let locked = $state(false)
  let field = $state('')
  let why = $state<CompareWhy | ''>('')
  let busy = $state(false)

  const run = $derived(shown?.run ?? null)
  const n = (x: number) => x.toLocaleString()
  const when = (ms: number) => new Date(ms).toLocaleString()

  async function load(offset = 0) {
    try {
      folders = await api.editableFolders()
      const found = await api.libraryCompare(offset, field || undefined, why || undefined, folder || undefined)
      shown = offset === 0 || !shown ? found : { ...found, songs: [...shown.songs, ...found.songs] }
      locked = false
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) locked = true
      else toast(e instanceof Error ? e.message : 'Could not load', true)
    }
  }

  async function unlock() {
    if (await askToUnlock()) await load()
  }

  async function start() {
    busy = true
    try {
      await api.startLibraryCompare()
      await load()
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) {
        locked = true
      } else toast(e instanceof Error ? e.message : 'Could not start', true)
    } finally {
      busy = false
    }
  }

  onMount(() => {
    void load()
    // Keep progress current while it runs.
    const timer = setInterval(() => {
      if (shown?.running) void load()
    }, 2000)
    return () => clearInterval(timer)
  })

  async function allow(f: EditableFolder, on: boolean) {
    try {
      await unlocked(() => api.setFolderEditable(f.path, on))
      folders = await api.editableFolders()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change it', true)
      folders = await api.editableFolders().catch(() => folders)
    }
  }

  const folderName = (path: string) => folders.find((f) => f.path === path)?.name ?? path
  const folderStats = (path: string) => run?.folders.find((x) => x.path === path)

  function pick(f: string, w: CompareWhy | '') {
    field = f
    why = w
    void load()
  }

  const total = $derived(run?.counts.reduce((a, c) => a + c.songs, 0) ?? 0)
</script>

<section class="card">
  <h3>Compare and folders</h3>
  <p class="small muted">
    Compare sets DJINN's database against what each file's tags say. Nothing is changed.
  </p>
  {#if locked}
    <p class="small">The editor is locked. <button class="link" onclick={unlock}>Unlock</button></p>
  {:else}
    <h4>Which folders can be changed</h4>
    <p class="small muted">
      Every library folder is view only until you allow changes in it. You can always see what's going on in a view-only
      folder (the compare below and the Problems page read every folder), but nothing in it can be changed. Use this for
      a folder someone else looks after. Allowing a folder checks that DJINN can write there.
    </p>
    <ul class="folders">
      {#each folders as f (f.path)}
        <li>
          <div>
            <strong>{f.name}</strong>
            <span class="badge" class:on={f.editable}>{f.editable ? 'Can be changed' : 'View only'}</span>
            <div class="small mono muted path">{f.path}</div>
          </div>
          <button class="small-button" onclick={() => allow(f, !f.editable)}>
            {f.editable ? 'Make view only' : 'Allow changes'}
          </button>
        </li>
      {/each}
    </ul>
    <h4>Compare the database with the files</h4>
    <div class="actions">
      <button class="primary" onclick={start} disabled={busy || shown?.running}>
        {run ? 'Compare again' : 'Compare now'}
      </button>
      {#if shown?.running && run}
        <span class="small"><span class="spinner" aria-hidden="true"></span> {n(run.done)} of {n(run.total)} songs</span>
      {/if}
    </div>
    <p class="small muted">
      Reads every song's tags from its file (about half a minute for 40,000 songs on a local disk, longer on a network
      share) and waits its turn like the other background jobs.
    </p>
    {#if run && !shown?.running}
      <p class="small">
        {#if run.error}<span class="bad">Stopped: {run.error}</span>{:else}
          {n(run.total)} songs compared on {when(run.finished_at ?? run.started_at)}:
          {#if run.differing === 0}
            <strong>the database matches every file.</strong>
          {:else}
            <strong>{n(run.differing)} {run.differing === 1 ? 'song differs' : 'songs differ'}</strong> from their files.
          {/if}
          {#if run.unreadable}{n(run.unreadable)} files couldn't be read.{/if}
        {/if}
      </p>
    {/if}
    {#if run && total > 0}
      <table class="counts">
        <thead>
          <tr><th>Field</th>{#each Object.keys(whyLabel) as w (w)}<th title={whyHelp[w as CompareWhy]}>{whyLabel[w as CompareWhy]}</th>{/each}</tr>
        </thead>
        <tbody>
          {#each [...new Set(run.counts.map((c) => c.field))] as f (f)}
            <tr>
              <td>{fieldLabel[f] ?? f}</td>
              {#each Object.keys(whyLabel) as w (w)}
                {@const c = run.counts.find((x) => x.field === f && x.why === w)}
                <td>
                  {#if c}<button class="link" onclick={() => pick(f, w as CompareWhy)}>{n(c.songs)}</button>{:else}<span class="muted">0</span>{/if}
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
      {#if run.folders.length > 1}
        <div class="actions">
          <label class="small">
            Folder
            <select bind:value={folder} onchange={() => load()}>
              <option value="">All folders</option>
              {#each run.folders as f (f.path)}
                <option value={f.path}>{folderName(f.path)} ({n(folderStats(f.path)?.differing ?? 0)} of {n(f.songs)} songs differ)</option>
              {/each}
            </select>
          </label>
        </div>
      {/if}
      <div class="actions">
        {#if field || why}<button class="small-button" onclick={() => pick('', '')}>Show all differences</button>{/if}
        <span class="small muted">{n(shown?.matching ?? 0)} songs shown{field ? ` for ${fieldLabel[field] ?? field}` : ''}{why ? `, ${whyLabel[why].toLowerCase()}` : ''}</span>
      </div>
      <ul class="songs">
        {#each shown?.songs ?? [] as song (song.track_id)}
          <li>
            <div><strong>{song.title}</strong>{#if song.artist}<span class="muted"> · {song.artist}</span>{/if}{#if run.folders.length > 1}<span class="muted small"> · {folderName(song.folder)}</span>{/if}</div>
            <div class="path small mono muted">{song.path}</div>
            {#each song.diffs as d (d.field)}
              <div class="diff small">
                <span class="field">{fieldLabel[d.field] ?? d.field}</span>
                <span>DJINN: <b>{d.database ?? '(empty)'}</b></span>
                <span>File: <b>{d.file ?? '(empty)'}</b></span>
                <span class="why">{whyLabel[d.why]}</span>
              </div>
            {/each}
          </li>
        {/each}
      </ul>
      {#if shown && shown.songs.length < shown.matching}
        <button class="small-button" onclick={() => load(shown?.songs.length ?? 0)}>Show more</button>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    margin: 8px 0;
  }

  .folders {
    list-style: none;
    margin: 8px 0;
    padding: 0;
  }

  .folders li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--line);
  }

  .badge.on {
    background: var(--accent);
    color: var(--on-accent, #000);
  }

  .counts {
    border-collapse: collapse;
    margin: 8px 0;
  }

  .counts th,
  .counts td {
    padding: 4px 12px 4px 0;
    text-align: left;
    font-size: 0.9rem;
  }

  .songs {
    list-style: none;
    margin: 8px 0;
    padding: 0;
  }

  .songs li {
    padding: 8px 0;
    border-top: 1px solid var(--line);
  }

  .path {
    word-break: break-all;
  }

  .diff {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 14px;
    margin-top: 2px;
  }

  .field {
    min-width: 7em;
    font-weight: 600;
  }

  .why {
    color: var(--muted);
  }

  .bad {
    color: var(--danger, #d33);
  }
</style>
