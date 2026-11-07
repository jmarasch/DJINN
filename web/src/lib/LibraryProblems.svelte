<script lang="ts">
  // Admin > Library > Problems: what the last library check couldn't add,
  // songs that won't play or whose tags couldn't be read, and gaps in the
  // tags, each with what to do about it. Kinds the admin already knows
  // about can be hidden.
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { parseReports, reportShort } from './reports'
  import type { ProblemCount, ProblemItem, ProblemKind, ProblemSummary } from './types'

  const about: Record<ProblemKind, { label: string; hint: string }> = {
    missing_folder: {
      label: 'Music folders not found',
      hint: "The folder isn't there right now. Plug the drive in or reconnect the share, or remove the folder above if it moved. Its songs stay in DJINN until it's back.",
    },
    unreadable: {
      label: "Folders and files DJINN can't open",
      hint: "DJINN wasn't allowed to read these. Check that the account DJINN runs as can open them, and that network shares are connected.",
    },
    bad_name: {
      label: "Names DJINN can't read",
      hint: 'These names use characters that are not valid text, often from an old computer or a ripped CD. Rename the file or folder, then check again.',
    },
    unsupported: {
      label: "Formats DJINN doesn't play",
      hint: 'DJINN plays MP3, FLAC, AAC and M4A, Ogg Vorbis and WAV. Convert these to FLAC (full quality) or MP3 with a converter, then check again.',
    },
    tags: {
      label: 'Songs with unreadable tags',
      hint: "DJINN couldn't read these files' tags, so they show under their file names. Fix the tags with a tag editor such as Mp3tag or MusicBrainz Picard, or use Edit tags on the song, which keeps your changes in DJINN without touching the file.",
    },
    wont_play: {
      label: "Songs that won't play",
      hint: 'These failed when played or checked, so the file is probably damaged or incomplete. Replace it with a good copy. DJINN tries again when the file changes.',
    },
    reported: {
      label: 'Songs listeners reported',
      hint: 'Listeners said these need fixing. Fix the song (Edit tags, a better copy, or a cover in its folder), then select Done to clear the reports.',
    },
    no_artist: {
      label: 'Songs with no artist',
      hint: 'Add the artist with Edit tags on the song, or in a tag editor.',
    },
    no_album: {
      label: 'Songs with no album',
      hint: 'These show as loose songs. Add the album with Edit tags on the song, or in a tag editor.',
    },
    album_no_cover: {
      label: 'Albums with no cover',
      hint: "Put a cover.jpg in the album's folder or embed a cover with a tag editor. With MusicBrainz on (Library extras, below), DJINN looks covers up.",
    },
    album_no_year: {
      label: 'Albums with no year',
      hint: 'Add the year with Edit album, or turn on MusicBrainz or Identify songs to fill years in.',
    },
    album_no_genre: {
      label: 'Albums with no genre',
      hint: 'Add a genre with Edit album, or turn on MusicBrainz or AI listening to fill genres in.',
    },
  }

  let summary = $state<ProblemSummary | null>(null)
  let open = $state<ProblemKind | null>(null)
  let items = $state<ProblemItem[]>([])
  let loading = $state(false)

  const shown = (k: ProblemCount) => k.count > 0 && !k.hidden
  const trouble = $derived(summary?.kinds.filter((k) => k.trouble && shown(k)) ?? [])
  const gaps = $derived(summary?.kinds.filter((k) => !k.trouble && shown(k)) ?? [])
  const hidden = $derived(summary?.kinds.filter((k) => k.hidden) ?? [])
  const openCount = $derived(summary?.kinds.find((k) => k.kind === open)?.count ?? 0)

  async function load() {
    try {
      summary = await api.libraryProblems()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load problems', true)
    }
  }

  $effect(() => {
    load()
  })

  // A finished library check may have changed everything.
  let lastDone: boolean | undefined
  $effect(() => {
    const done = app.scan?.done
    if (done && lastDone === false) {
      load()
      if (open) show(open, true)
    }
    lastDone = done
  })

  async function show(kind: ProblemKind, again = false) {
    if (open === kind && !again) {
      open = null
      return
    }
    open = kind
    items = []
    await more()
  }

  async function more() {
    if (!open) return
    loading = true
    try {
      const kind = open
      const page = await api.libraryProblemList(kind, items.length)
      if (open === kind) items = [...items, ...page]
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the list', true)
    } finally {
      loading = false
    }
  }

  async function done(item: ProblemItem) {
    if (item.track_id === null) return
    try {
      await api.clearSongReports(item.track_id)
      items = items.filter((i) => i !== item)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not clear the reports', true)
    }
  }

  async function setHidden(kind: ProblemKind, hide: boolean) {
    if (!summary) return
    const kinds = summary.kinds.filter((k) => (k.kind === kind ? hide : k.hidden)).map((k) => k.kind)
    try {
      await api.hideLibraryProblems(kinds)
      if (hide && open === kind) open = null
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save that', true)
    }
  }

  async function checkAgain() {
    try {
      const { status } = await api.rescan()
      toast(status === 'started' ? 'Checking for changes' : 'Already checking for changes')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the check', true)
    }
  }

  const n = (x: number) => x.toLocaleString()
  const folder = (path: string) => path.replace(/[\\/][^\\/]*$/, '')
  const name = (path: string) => path.replace(/^.*[\\/]/, '')
</script>

{#snippet kindRow(k: ProblemCount)}
  <div class="kind">
    <button class="row" class:open={open === k.kind} aria-expanded={open === k.kind} onclick={() => show(k.kind)}>
      <span class="chevron" aria-hidden="true"></span>
      <span class="grow">{about[k.kind].label}</span>
      <span class="count">{n(k.count)}</span>
    </button>
    {#if open === k.kind}
      <div class="detail">
        <p class="small muted">{about[k.kind].hint}</p>
        <ul class="items">
          {#each items as item, i (i)}
            <li>
              {#if k.per_album}
                <a href="#/album/{item.album_id}">{item.album}</a>
                <span class="muted"> · {item.artist}</span>
                {#if item.path}<div class="path small mono">{folder(item.path)}</div>{/if}
              {:else if item.track_id !== null}
                {#if item.album_id !== null}
                  <a href="#/album/{item.album_id}">{item.title}</a>
                {:else}
                  <span>{item.title}</span>
                {/if}
                {#if item.artist}<span class="muted"> · {item.artist}</span>{/if}
                <div class="path small mono">{item.path}</div>
                {#if k.kind === 'reported'}
                  {#each parseReports(item.detail) as report, r (r)}
                    <div class="small">
                      {reportShort[report.reason] ?? report.reason}{#if report.note}: <span class="muted">{report.note}</span
                        >{/if}
                    </div>
                  {/each}
                  <button class="small-button done" onclick={() => done(item)}>Done</button>
                {:else if item.detail}<div class="small muted">{item.detail}</div>{/if}
              {:else}
                {#if item.path}
                  <div>{name(item.path)}</div>
                  <div class="path small mono">{folder(item.path)}</div>
                {/if}
                {#if item.detail && k.kind !== 'unsupported'}<div class="small muted">{item.detail}</div>{/if}
              {/if}
            </li>
          {/each}
        </ul>
        <div class="actions">
          {#if items.length < openCount}
            <button class="small-button" disabled={loading} onclick={more}>Show more</button>
          {/if}
          <button class="small-button" onclick={() => setHidden(k.kind, true)}>Hide this kind</button>
        </div>
      </div>
    {/if}
  </div>
{/snippet}

<section class="card">
  <h2>
    Problems
    {#if summary && summary.trouble > 0}<span class="badge">{n(summary.trouble)}</span>{/if}
  </h2>
  <p class="small muted">
    What the last library check couldn't add or found broken, and gaps in your tags. Only music files are checked;
    pictures, playlists and other files in your folders are left alone. Select a line to see its files. Your music
    folders are never changed.
  </p>
  {#if !summary}
    <p class="small muted">Loading...</p>
  {:else}
    {#if trouble.length}
      <h3>Needs a look</h3>
      {#each trouble as k (k.kind)}{@render kindRow(k)}{/each}
    {:else}
      <p class="small">Every file in your music folders made it in and plays.</p>
    {/if}
    {#if gaps.length}
      <h3>Gaps in the tags</h3>
      {#each gaps as k (k.kind)}{@render kindRow(k)}{/each}
    {/if}
    {#if hidden.length}
      <p class="small muted">
        Hidden:
        {#each hidden as k, i (k.kind)}{#if i > 0},
          {/if}<button class="link" onclick={() => setHidden(k.kind, false)}
            >{about[k.kind].label} ({n(k.count)})</button
          >{/each}. Select one to show it again.
      </p>
    {/if}
    <div class="actions">
      <a class="button small-button" href="/api/library/problems.csv" download>Download as a spreadsheet</a>
      <button class="small-button" onclick={checkAgain}>Check again</button>
    </div>
  {/if}
</section>

<style>
  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h3 {
    margin: 8px 0 0;
    font-size: 0.95rem;
  }

  .badge {
    min-width: 22px;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--bad);
    color: #fff;
    font-size: 0.8rem;
    text-align: center;
  }

  .kind {
    border-bottom: 1px solid var(--line);
  }

  .row {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 8px;
    padding: 10px 4px;
    background: none;
    color: inherit;
    text-align: left;
  }

  .row:hover {
    background: var(--surface-2);
  }

  .row.open {
    font-weight: 600;
  }

  /* A small arrow that turns down when the list is open. */
  .chevron {
    flex: none;
    width: 8px;
    height: 8px;
    margin: 0 4px 0 2px;
    border-right: 2px solid var(--muted);
    border-bottom: 2px solid var(--muted);
    transform: rotate(-45deg);
    transition: transform 0.15s;
  }

  .row.open .chevron {
    transform: rotate(45deg);
  }

  .grow {
    flex: 1;
  }

  .count {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .detail {
    padding: 0 4px 12px;
  }

  .items {
    margin: 8px 0;
    padding: 0;
    list-style: none;
  }

  .items li {
    padding: 6px 0;
    border-top: 1px solid var(--line);
    overflow-wrap: anywhere;
  }

  /* List rows otherwise get plain buttons. */
  .items .done {
    margin-top: 6px;
    background: var(--surface-2);
  }

  .path {
    color: var(--muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .link {
    display: inline;
    min-height: 0;
    padding: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
  }
</style>
