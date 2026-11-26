<script lang="ts">
  import { findInEditor, openInEditor } from './editorJump.svelte'
  // Admin > Library > Problems: what the last library check couldn't add,
  // songs that won't play or whose tags couldn't be read, and gaps in the
  // tags, each with what to do about it. Kinds the admin already knows
  // about can be hidden.
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import { parseReports, reportShort } from './reports'
  import QuarantineWindow from './QuarantineWindow.svelte'
  import type { Mp3Check, ProblemCount, ProblemItem, ProblemKind, ProblemSummary } from './types'

  /** A problem kind's name and what to do about it (`prob.<kind>`, `prob.<kind>_hint`). */
  const about = (kind: ProblemKind) => ({ label: t(`prob.${kind}`), hint: t(`prob.${kind}_hint`) })

  let summary = $state<ProblemSummary | null>(null)
  let open = $state<ProblemKind | null>(null)
  let items = $state<ProblemItem[]>([])
  let loading = $state(false)
  /** The Quarantine window is open. */
  let quarantining = $state(false)

  const shown = (k: ProblemCount) => k.count > 0 && !k.hidden
  const trouble = $derived(summary?.kinds.filter((k) => k.trouble && shown(k)) ?? [])
  const gaps = $derived(summary?.kinds.filter((k) => !k.trouble && shown(k)) ?? [])
  const hidden = $derived(summary?.kinds.filter((k) => k.hidden) ?? [])
  const openCount = $derived(summary?.kinds.find((k) => k.kind === open)?.count ?? 0)

  async function load() {
    try {
      summary = await api.libraryProblems()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('prob.load_failed'), true)
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
      toast(e instanceof Error ? e.message : t('prob.list_failed'), true)
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
      peeking = { ...peeking, [id]: text || t('prob.no_lyrics') }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('prob.lyrics_failed'), true)
    }
  }

  async function removeLyrics(item: ProblemItem) {
    if (item.track_id === null) return
    if (!confirm(t('prob.remove_lyrics_q', { title: item.title ?? '' }))) return
    try {
      await api.blockLyrics(item.track_id)
      items = items.filter((i) => i !== item)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('prob.remove_lyrics_failed'), true)
    }
  }

  async function done(item: ProblemItem) {
    if (item.track_id === null) return
    try {
      await api.clearSongReports(item.track_id)
      items = items.filter((i) => i !== item)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('prob.clear_failed'), true)
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
      toast(e instanceof Error ? e.message : t('prob.save_failed'), true)
    }
  }

  async function checkAgain() {
    try {
      const { status } = await api.rescan()
      toast(status === 'started' ? t('admin.checking') : t('admin.already_checking'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('admin.check_failed'), true)
    }
  }

  const n = (x: number) => x.toLocaleString(locale.code)
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
      toast(t('prob.mp3_again'))
      await loadMp3()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.start_failed'), true)
    }
  }

  async function measure() {
    try {
      await api.measureLoudness()
      toast(t('prob.measuring'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.start_failed'), true)
    }
  }

  const folder = (path: string) => path.replace(/[\\/][^\\/]*$/, '')
  const name = (path: string) => path.replace(/^.*[\\/]/, '')
</script>

{#snippet kindRow(k: ProblemCount)}
  <div class="kind">
    <button class="row" class:open={open === k.kind} aria-expanded={open === k.kind} onclick={() => show(k.kind)}>
      <span class="chevron" aria-hidden="true"></span>
      <span class="grow">{about(k.kind).label}</span>
      <span class="count">{n(k.count)}</span>
    </button>
    {#if open === k.kind}
      <div class="detail">
        <p class="small muted">{about(k.kind).hint}</p>
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
                    title={t('prob.compare_title')}
                    onclick={() => findInEditor([item.album ?? '', /Also filed as "([^"]+)"/.exec(item.detail)?.[1] ?? ''])}
                    >{t('prob.compare')}</button
                  >
                {:else if item.path}
                  <button class="small-button done" onclick={() => openInEditor(folder(item.path ?? ''))}>{t('prob.fix')}</button>
                {:else if item.artist}
                  <button class="small-button done" onclick={() => findInEditor([item.album ?? item.artist ?? ''])}>{t('prob.fix')}</button>
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
                      {reportShort(report.reason)}{#if report.note}: <span class="muted">{report.note}</span
                        >{/if}
                    </div>
                  {/each}
                  <button class="small-button done" onclick={() => done(item)}>{t('prob.done')}</button>
                {:else if k.kind === 'reported_lyrics'}
                  {#each parseReports(item.detail) as report, r (r)}
                    <div class="small">
                      {reportShort(report.reason)}{#if report.note}: <span class="muted">{report.note}</span
                        >{/if}
                    </div>
                  {/each}
                  {#if item.track_id !== null && item.track_id in peeking}
                    <pre class="peek small">{peeking[item.track_id]}</pre>
                  {/if}
                  <button class="small-button done" onclick={() => peek(item)}>{t('prob.show_lyrics')}</button>
                  <button class="small-button done danger" onclick={() => removeLyrics(item)}>{t('prob.remove_block')}</button>
                  <button class="small-button done" onclick={() => done(item)}>{t('prob.done')}</button>
                {:else if item.detail}<div class="small muted">{item.detail}</div>{/if}
                {#if k.kind === 'low_bitrate' && item.title}
                  <button
                    class="small-button done"
                    title={t('prob.better_title')}
                    onclick={() => findInEditor([item.title ?? ''])}>{t('prob.better')}</button
                  >
                {/if}
                {#if k.kind === 'vanished' && item.title}
                  <button
                    class="small-button done"
                    title={t('prob.look_title')}
                    onclick={() => findInEditor([item.title ?? ''])}>{t('prob.look')}</button
                  >
                {:else if item.path && k.kind !== 'reported_lyrics'}
                  <button class="small-button done" onclick={() => openInEditor(folder(item.path ?? ''))}>{t('prob.fix')}</button>
                {/if}
              {:else}
                {#if item.path}
                  <div>{name(item.path)}</div>
                  <div class="path small mono">{folder(item.path)}</div>
                {/if}
                {#if item.detail && k.kind !== 'unsupported'}<div class="small muted">{item.detail}</div>{/if}
                {#if k.kind === 'artist_variants' && item.detail}
                  <button class="small-button done" onclick={() => findInEditor(item.detail.split('/'))}>{t('prob.fix')}</button>
                {/if}
              {/if}
            </li>
          {/each}
        </ul>
        <div class="actions">
          {#if items.length < openCount}
            <button class="small-button" disabled={loading} onclick={more}>{t('common.show_more')}</button>
          {/if}
          {#if k.kind === 'no_loudness'}
            <button class="small-button" onclick={measure}>{t('prob.measure')}</button>
          {/if}
          {#if k.kind === 'no_length_header'}
            <button class="small-button" onclick={mp3Again}>{t('prob.read_again')}</button>
          {/if}
          <button class="small-button" onclick={() => setHidden(k.kind, true)}>{t('prob.hide')}</button>
        </div>
      </div>
    {/if}
  </div>
{/snippet}

{#if quarantining && summary}
  <QuarantineWindow
    problems={summary.kinds.filter((k) => k.songs && k.count > 0)}
    onclose={() => (quarantining = false)}
    ondone={() => {
      load()
      if (open) show(open, true)
    }}
  />
{/if}

<section class="card">
  <h2>
    {t('prob.title')}
    {#if summary && summary.trouble > 0}<span class="badge">{n(summary.trouble)}</span>{/if}
  </h2>
  <p class="small muted">
    {t('prob.intro')}
  </p>
  {#if !summary}
    <p class="small muted">{t('common.loading')}</p>
  {:else}
    {#if trouble.length}
      <h3>{t('prob.needs')}</h3>
      {#each trouble as k (k.kind)}{@render kindRow(k)}{/each}
    {:else}
      <p class="small">{t('prob.all_good')}</p>
    {/if}
    {#if gaps.length}
      <h3>{t('prob.worth')}</h3>
      {#each gaps as k (k.kind)}{@render kindRow(k)}{/each}
    {/if}
    {#if hidden.length}
      <p class="small muted">
        {t('prob.hidden')}
        {#each hidden as k, i (k.kind)}{#if i > 0},
          {/if}<button class="link" onclick={() => setHidden(k.kind, false)}
            >{about(k.kind).label} ({n(k.count)})</button
          >{/each}{t('prob.show_again')}
      </p>
    {/if}
    {#if mp3 && mp3.total}
      <p class="small muted">
        {t('prob.thorough', { done: n(mp3.checked), total: n(mp3.total) })}{#if mp3.running}{t('prob.working')}{/if}{#if mp3.missing}{t(
            'prob.missing',
            { n: n(mp3.missing) },
          )}{/if}.
      </p>
    {/if}
    <div class="actions">
      <a class="button small-button" href="/api/library/problems.csv" download>{t('prob.csv')}</a>
      <button class="small-button" title={t('prob.quarantine_title')} onclick={() => (quarantining = true)}
        >{t('prob.quarantine')}</button
      >
      <button class="small-button" onclick={checkAgain}>{t('prob.check_again')}</button>
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
