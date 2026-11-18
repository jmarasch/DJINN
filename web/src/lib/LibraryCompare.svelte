<script lang="ts">
  // Admin > Library Editor > Compare: DJINN's database against the files.
  // Compares what the database holds with what each file's own tags say,
  // field by field, and why they differ.
  // It only reads: nothing in the music folders or the database changes.
  // The editor sits behind the protected areas' lock.
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import { askToUnlock } from './protected.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { CompareShown, CompareWhy, EditableFolder } from './types'

  const whyLabel: Record<CompareWhy, string> = $derived({
    correction: t('cmp.why_correction'),
    identified: t('cmp.why_identified'),
    other: t('cmp.why_other'),
  })
  const whyHelp: Record<CompareWhy, string> = $derived({
    correction: t('cmp.help_correction'),
    identified: t('cmp.help_identified'),
    other: t('cmp.help_other'),
  })
  const fieldLabel: Record<string, string> = $derived({
    title: t('ed.title'),
    artist: t('ed.artist'),
    album: t('ed.album'),
    album_artist: t('ed.album_artist'),
    track_no: t('cmp.track_number'),
    disc_no: t('cmp.disc_number'),
    year: t('ed.year'),
    genre: t('ed.genre'),
  })

  let shown = $state<CompareShown | null>(null)
  let folders = $state<EditableFolder[]>([])
  let folder = $state('')
  let locked = $state(false)
  let field = $state('')
  let why = $state<CompareWhy | ''>('')
  let busy = $state(false)

  const run = $derived(shown?.run ?? null)
  const n = (x: number) => x.toLocaleString(locale.code)
  const when = (ms: number) => new Date(ms).toLocaleString(locale.code)

  async function load(offset = 0) {
    try {
      folders = await api.editableFolders()
      const found = await api.libraryCompare(offset, field || undefined, why || undefined, folder || undefined)
      shown = offset === 0 || !shown ? found : { ...found, songs: [...shown.songs, ...found.songs] }
      locked = false
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) locked = true
      else toast(e instanceof Error ? e.message : t('ed.load_failed'), true)
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
      } else toast(e instanceof Error ? e.message : t('common.start_failed'), true)
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
  <h3>{t('cmp.title')}</h3>
  <p class="small muted">
    {t('cmp.intro')}
  </p>
  {#if locked}
    <p class="small">{t('ed.locked')} <button class="link" onclick={unlock}>{t('ed.unlock')}</button></p>
  {:else}
    <h4>{t('cmp.heading')}</h4>
    <div class="actions">
      <button class="primary" onclick={start} disabled={busy || shown?.running}>
        {run ? t('cmp.again') : t('cmp.now')}
      </button>
      {#if shown?.running && run}
        <span class="small"><span class="spinner" aria-hidden="true"></span> {t('cmp.progress', { done: n(run.done), total: n(run.total) })}</span>
      {/if}
    </div>
    <p class="small muted">
      {t('cmp.note')}
    </p>
    {#if run && !shown?.running}
      <p class="small">
        {#if run.error}<span class="bad">{t('cmp.stopped', { error: run.error })}</span>{:else}
          {t('cmp.compared', { n: n(run.total), when: when(run.finished_at ?? run.started_at) })}
          {#if run.differing === 0}
            <strong>{t('cmp.matches')}</strong>
          {:else}
            <strong>{t('cmp.differ', { n: run.differing })}</strong> {t('cmp.from_files')}
          {/if}
          {#if run.unreadable}{t('cmp.unreadable', { n: n(run.unreadable) })}{/if}
        {/if}
      </p>
    {/if}
    {#if run && total > 0}
      <table class="counts">
        <thead>
          <tr><th>{t('cmp.field')}</th>{#each Object.keys(whyLabel) as w (w)}<th title={whyHelp[w as CompareWhy]}>{whyLabel[w as CompareWhy]}</th>{/each}</tr>
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
            {t('cmp.folder')}
            <select bind:value={folder} onchange={() => load()}>
              <option value="">{t('cmp.all_folders')}</option>
              {#each run.folders as f (f.path)}
                <option value={f.path}
                  >{t('cmp.folder_option', { name: folderName(f.path), differ: n(folderStats(f.path)?.differing ?? 0), songs: n(f.songs) })}</option
                >
              {/each}
            </select>
          </label>
        </div>
      {/if}
      <div class="actions">
        {#if field || why}<button class="small-button" onclick={() => pick('', '')}>{t('cmp.show_all')}</button>{/if}
        <span class="small muted"
          >{t('cmp.shown', { n: n(shown?.matching ?? 0) })}{field ? t('cmp.for_field', { field: fieldLabel[field] ?? field }) : ''}{why
            ? `, ${whyLabel[why].toLowerCase()}`
            : ''}</span
        >
      </div>
      <ul class="songs">
        {#each shown?.songs ?? [] as song (song.track_id)}
          <li>
            <div><strong>{song.title}</strong>{#if song.artist}<span class="muted"> · {song.artist}</span>{/if}{#if run.folders.length > 1}<span class="muted small"> · {folderName(song.folder)}</span>{/if}</div>
            <div class="path small mono muted">{song.path}</div>
            {#each song.diffs as d (d.field)}
              <div class="diff small">
                <span class="field">{fieldLabel[d.field] ?? d.field}</span>
                <span>{t('cmp.djinn')} <b>{d.database ?? t('ed.empty')}</b></span>
                <span>{t('cmp.file')} <b>{d.file ?? t('ed.empty')}</b></span>
                <span class="why">{whyLabel[d.why]}</span>
              </div>
            {/each}
          </li>
        {/each}
      </ul>
      {#if shown && shown.songs.length < shown.matching}
        <button class="small-button" onclick={() => load(shown?.songs.length ?? 0)}>{t('common.show_more')}</button>
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
