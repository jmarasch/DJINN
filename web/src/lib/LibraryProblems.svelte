<script lang="ts">
  import { findInEditor, openInEditor } from './editorJump.svelte'
  // Admin > Library > Problems: what the last library check couldn't add,
  // songs that won't play or whose tags couldn't be read, and gaps in the
  // tags, each with what to do about it. Kinds the admin already knows
  // about can be hidden.
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { parseReports, reportShort } from './reports'
  import type { Mp3Check, ProblemCount, ProblemItem, ProblemKind, ProblemSummary } from './types'

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
    reported_lyrics: {
      label: 'Lyrics listeners reported',
      hint: 'Listeners said these lyrics are wrong or bad. Remove and block takes them away and remembers where they came from, so that source is not used again for the song unless its text changes. Done clears the reports and keeps the lyrics.',
    },
    duplicate_albums: {
      label: 'Albums filed twice',
      hint: "These albums by one artist hold almost the same songs: a copy in another folder, or the same album under a slightly different name. Keep the better one and remove the other from your music folders, then check for changes. A deluxe edition with extra songs isn't listed, and neither is a song that sits on several albums.",
    },
    identical_files: {
      label: 'The same file twice in one album',
      hint: 'These songs in one album have the same size, length and title, so they are almost certainly the same file copied. Keep one and remove the other from your music folders, then check for changes.',
    },
    bogus_values: {
      label: 'Songs with impossible numbers',
      hint: 'A track or disc number of 0 or less, or a length of 0, usually means a bad rip or a damaged tag. Fix it with Edit tags or a tag editor. For an MP3 with a length of 0, see the note under "Very short or very long songs".',
    },
    odd_length: {
      label: 'Very short or very long songs',
      hint: 'Under 10 seconds or over 3 hours. Often a cut-off download, a hidden track, a skit, or a whole mix filed as one song. A variable bit rate MP3 with no length header can also show a wrong length in other players, even though DJINN counts it right. To fix the file: select Fix in the editor, allow the folder under the folders DJINN may change (Admin > Library > Library Editor), unlock the protected area, pick the MP3s, and choose Fix MP3 lengths. It adds the header and can be undone from the editor\'s history.',
    },
    low_bitrate: {
      label: 'Low-quality songs',
      hint: 'Under 96 kbps, which sounds poor. If you have a better copy, replace the file.',
    },
    title_junk: {
      label: 'Titles with leftovers',
      hint: 'A track number in front, "(Official Video)", a web address, spaces around, or all capitals. Clean it with Edit tags, which keeps your change in DJINN, or in a tag editor.',
    },
    name_mismatch: {
      label: "File names that don't match the title",
      hint: "The file's name (and folders) don't contain the song's title. Sometimes the tag is right and the name is just different (accents and punctuation count), sometimes the file is mislabeled. Play it to check.",
    },
    artist_variants: {
      label: 'Artists spelled two ways',
      hint: 'Names that differ only by capitals, a leading "The", "&" or punctuation, so one artist shows twice ("The Beatles" and "Beatles", "AC/DC" and "ACDC"). Pick one spelling and fix the other with Edit tags.',
    },
    split_across_artists: {
      label: 'Albums split across artist names',
      hint: "These albums share a folder and a title but were filed under different artist names, often because of a compilation tag or a \"feat.\" in the album artist. Set the same album artist on all of its songs with Edit album.",
    },
    cover_differs: {
      label: 'Albums with different covers inside',
      hint: "The songs of these albums carry more than one cover picture. Embed the same cover in every song, or put one cover.jpg in the folder.",
    },
    track_gaps: {
      label: 'Albums with missing or repeated track numbers',
      hint: 'Track numbers that start at 1 with a hole in them (songs missing from the album), or one number used twice. Check that all songs are there and numbered right.',
    },
    no_lyrics: {
      label: 'Songs with no lyrics',
      hint: 'No lyrics were found in a .lrc file, the tags, an import or LRCLIB. Turn on LRCLIB lookups or import a zip in Library > Lyrics. Instrumentals are fine.',
    },
    no_loudness: {
      label: 'Songs with no loudness data',
      hint: 'No ReplayGain tags and no measurement, in a library where other songs have them. Turn on ReplayGain in Audio so DJINN measures them.',
    },
    ai_lyrics: {
      label: 'Lyrics an AI made',
      hint: 'An AI worked on these lyrics. "Words made by an AI" are guesses from listening and may have mistakes: any other source replaces them (LRCLIB is still asked about these songs, and an import of ordinary lyrics replaces them). "Timed by an AI" are a person\'s words with timing worked out by an AI; the match says how well they fit, so check the low ones. Each row says which import it came from. Use Report these lyrics, then Remove and block, on one that is wrong.',
    },
    messy_genres: {
      label: 'Messy genre tags',
      hint: 'Genre tags that are not one clean genre: several run together with ";" or "|" ("rock;alternative pop rock"), an old genre number like "(17)", a placeholder like "Unknown" or "Other", or a whole sentence. Each one shows as a genre of its own in genre lists, the genre cloud and genre containers. To fix them: select Fix in the editor, pick the songs, choose Genre in the tools under the grid, type the genres with commas between them ("Rock, Alternative") and select Set, then Approve. Editing the Database keeps the change in DJINN only; editing the Library writes it into the files (the folder must be allowed under the folders DJINN may change).',
    },
    no_length_header: {
      label: 'MP3s with no length header',
      hint: "These variable bit rate MP3s have no length header, so many players, phones and car stereos show the wrong length and can't skip ahead properly. DJINN counts them right. To fix the files: select Fix in the editor, allow the folder under the folders DJINN may change (Admin > Library > Library Editor), unlock the protected area, pick the MP3s, and choose Fix MP3 lengths. It adds the header without touching the audio and can be undone from the editor's history. The thorough check reads the start of every MP3 in the background, and again whenever one changes.",
    },
    vanished: {
      label: 'Songs that went missing lately',
      hint: 'These were in the library and their files are gone (in the last 30 days). If that is a mistake, plug the drive back in or restore the files, then check for changes.',
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

  /** Lyrics shown for a reported song, by song id. */
  let peeking = $state<Record<number, string>>({})

  async function peek(item: ProblemItem) {
    const id = item.track_id
    if (id === null) return
    if (id in peeking) {
      const { [id]: _gone, ...rest } = peeking
      peeking = rest
      return
    }
    try {
      const found = await api.trackLyrics(id)
      const text = found?.lines ? found.lines.map((l) => l.text).join('\n') : (found?.plain ?? '')
      peeking = { ...peeking, [id]: text || 'No lyrics.' }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the lyrics', true)
    }
  }

  async function removeLyrics(item: ProblemItem) {
    if (item.track_id === null) return
    if (!confirm(`Remove the lyrics of "${item.title}"? They won't be taken from the same source again.`)) return
    try {
      await api.blockLyrics(item.track_id)
      items = items.filter((i) => i !== item)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove the lyrics', true)
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
  let mp3 = $state<Mp3Check | null>(null)

  async function loadMp3() {
    try {
      mp3 = await api.mp3Check()
    } catch {
      // Shown next time.
    }
  }

  $effect(() => {
    void loadMp3()
    const timer = setInterval(() => {
      if (mp3?.running) void loadMp3().then(load)
    }, 5000)
    return () => clearInterval(timer)
  })

  async function mp3Again() {
    try {
      await api.mp3CheckAgain()
      toast('Reading every MP3 again in the background')
      await loadMp3()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start it', true)
    }
  }

  async function measure() {
    try {
      await api.measureLoudness()
      toast('Measuring in the background; the Library tab shows how far it got')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start it', true)
    }
  }

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
                {#if k.kind === 'duplicate_albums' && item.detail}<div class="small muted">{item.detail}</div>{/if}
                {#if k.kind === 'duplicate_albums'}
                  <button
                    class="small-button done"
                    title="Shows both albums' songs together in the library editor; Set aside the copy you don't keep"
                    onclick={() => findInEditor([item.album ?? '', /Also filed as "([^"]+)"/.exec(item.detail)?.[1] ?? ''])}
                    >Compare in the editor</button
                  >
                {:else if item.path}
                  <button class="small-button done" onclick={() => openInEditor(folder(item.path ?? ''))}>Fix in the editor</button>
                {:else if item.artist}
                  <button class="small-button done" onclick={() => findInEditor([item.album ?? item.artist ?? ''])}>Fix in the editor</button>
                {/if}
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
                {:else if k.kind === 'reported_lyrics'}
                  {#each parseReports(item.detail) as report, r (r)}
                    <div class="small">
                      {reportShort[report.reason] ?? report.reason}{#if report.note}: <span class="muted">{report.note}</span
                        >{/if}
                    </div>
                  {/each}
                  {#if item.track_id !== null && item.track_id in peeking}
                    <pre class="peek small">{peeking[item.track_id]}</pre>
                  {/if}
                  <button class="small-button done" onclick={() => peek(item)}>Show lyrics</button>
                  <button class="small-button done danger" onclick={() => removeLyrics(item)}>Remove and block</button>
                  <button class="small-button done" onclick={() => done(item)}>Done</button>
                {:else if item.detail}<div class="small muted">{item.detail}</div>{/if}
                {#if k.kind === 'low_bitrate' && item.title}
                  <button
                    class="small-button done"
                    title="Finds songs with this title in the library editor, to keep the better copy (Set aside the other) or Replace the file"
                    onclick={() => findInEditor([item.title ?? ''])}>Look for a better copy</button
                  >
                {/if}
                {#if k.kind === 'vanished' && item.title}
                  <button
                    class="small-button done"
                    title="Finds songs with this title in the library editor, in case it came back retagged or re-ripped"
                    onclick={() => findInEditor([item.title ?? ''])}>Look for it</button
                  >
                {:else if item.path && k.kind !== 'reported_lyrics'}
                  <button class="small-button done" onclick={() => openInEditor(folder(item.path ?? ''))}>Fix in the editor</button>
                {/if}
              {:else}
                {#if item.path}
                  <div>{name(item.path)}</div>
                  <div class="path small mono">{folder(item.path)}</div>
                {/if}
                {#if item.detail && k.kind !== 'unsupported'}<div class="small muted">{item.detail}</div>{/if}
                {#if k.kind === 'artist_variants' && item.detail}
                  <button class="small-button done" onclick={() => findInEditor(item.detail.split('/'))}>Fix in the editor</button>
                {/if}
              {/if}
            </li>
          {/each}
        </ul>
        <div class="actions">
          {#if items.length < openCount}
            <button class="small-button" disabled={loading} onclick={more}>Show more</button>
          {/if}
          {#if k.kind === 'no_loudness'}
            <button class="small-button" onclick={measure}>Measure now</button>
          {/if}
          {#if k.kind === 'no_length_header'}
            <button class="small-button" onclick={mp3Again}>Read every MP3 again</button>
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
      <h3>Worth a look</h3>
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
    {#if mp3 && mp3.total}
      <p class="small muted">
        Thorough check: {n(mp3.checked)} of {n(mp3.total)} MP3s read{#if mp3.running} (working on it){/if}{#if mp3.missing},
          {n(mp3.missing)} with no length header{/if}.
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
  .peek {
    max-height: 200px;
    overflow: auto;
    margin: 6px 0;
    padding: 6px 8px;
    white-space: pre-wrap;
    border: 1px solid var(--line);
    border-radius: 6px;
  }

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
