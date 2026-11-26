<script lang="ts">
  // The editor's Fix MP3 Lengths: finds which MP3s need a length header (the
  // songs picked, else every song in the folder or search, not only those
  // loaded in the grid), asks once with how many, then writes them a few at
  // a time with a progress bar. Every part joins one approval, so the
  // editor's history undoes the whole fix at once.
  import { onMount } from 'svelte'
  import { api } from './api'
  import FloatingWindow from './FloatingWindow.svelte'
  import { locale, t } from './i18n.svelte'
  import { unlocked } from './protected.svelte'
  import { toast } from './state.svelte'

  let {
    ids = [],
    folder,
    search = '',
    onclose,
    ondone,
  }: {
    /** The songs picked; empty means every song in `folder` or `search`. */
    ids?: number[]
    folder: string
    search?: string
    onclose: () => void
    ondone: () => void
  } = $props()

  /** Files read for the check, and written, per request. */
  const CHECK_STEP = 250
  const FIX_STEP = 10

  type Stage = 'finding' | 'checking' | 'ask' | 'none' | 'working'
  let stage = $state<Stage>('finding')
  let done = $state(0)
  let total = $state(0)
  /** The MP3s that need a header. */
  let needing = $state<number[]>([])
  let stopping = $state(false)
  let closed = false

  const n = (x: number) => x.toLocaleString(locale.code)

  onMount(() => {
    void find()
    return () => (closed = true)
  })

  /** Every song the fix is for, then which of them need it. */
  async function find() {
    try {
      let songs = ids
      if (!songs.length) {
        const all: number[] = []
        for (let offset = 0; ; ) {
          const page = await api.editorSongs(folder, offset, false, search)
          total = page.total
          for (const s of page.songs) if (s.editable && /\.mp3$/i.test(s.path)) all.push(s.id)
          offset += page.songs.length
          done = offset
          if (closed || !page.songs.length || offset >= page.total) break
        }
        songs = all
      }
      stage = 'checking'
      done = 0
      total = songs.length
      const found: number[] = []
      for (let i = 0; i < songs.length && !closed; i += CHECK_STEP) {
        const part = songs.slice(i, i + CHECK_STEP)
        const { files } = await unlocked(() => api.editorMp3Length(part, true))
        for (const f of files) if (!f.problem) found.push(f.id)
        done = Math.min(i + CHECK_STEP, songs.length)
      }
      needing = found
      stage = found.length ? 'ask' : 'none'
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.where_failed'), true)
      onclose()
    }
  }

  async function fix() {
    stage = 'working'
    done = 0
    total = needing.length
    let batch: number | null = null
    let fixed = 0
    const failed: string[] = []
    try {
      for (let i = 0; i < needing.length && !stopping && !closed; i += FIX_STEP) {
        const part = needing.slice(i, i + FIX_STEP)
        const answer = await unlocked(() => api.editorMp3Length(part, false, batch))
        batch = answer.batch ?? batch
        for (const f of answer.files) {
          if (f.problem) failed.push(f.problem)
          else fixed++
        }
        done = Math.min(i + FIX_STEP, needing.length)
      }
      // Files that turned out fine already count as fine, not as failures.
      const real = failed.filter((why) => why !== 'its length is right already')
      toast(
        real.length ? t('mp3w.partial', { n: n(fixed), failed: n(real.length), why: real[0] }) : t('ed.mv_done_mp3', { n: n(fixed) }),
        real.length > 0,
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.rename_failed'), true)
    }
    ondone()
    onclose()
  }
</script>

<FloatingWindow title={t('mp3w.title')} onclose={() => (stage === 'working' ? (stopping = true) : onclose())}>
  {#if stage === 'finding' || stage === 'checking'}
    <p><span class="spinner"></span>{stage === 'finding' ? t('mp3w.finding') : t('mp3w.checking', { done: n(done), total: n(total) })}</p>
    <progress max={stage === 'checking' ? total : undefined} value={stage === 'checking' ? done : undefined}></progress>
  {:else if stage === 'none'}
    <p>{t('ed.mp3_ok')}</p>
  {:else if stage === 'ask'}
    <p>{t('mp3w.ask', { n: n(needing.length) })}</p>
    <p class="small muted">{t('mp3w.note')}</p>
  {:else}
    <p><span class="spinner"></span>{t('mp3w.working', { done: n(done), total: n(total) })}</p>
    <progress max={total} value={done}></progress>
    <p class="small muted">{t('mp3w.dont_close')}</p>
  {/if}
  {#snippet footer()}
    {#if stage === 'ask'}
      <button onclick={onclose}>{t('common.cancel')}</button>
      <button class="primary" onclick={fix}>{t('mp3w.continue')}</button>
    {:else if stage === 'working'}
      <button disabled={stopping} onclick={() => (stopping = true)}>{t('common.stop')}</button>
    {:else if stage === 'none'}
      <button class="primary" onclick={onclose}>{t('common.close')}</button>
    {:else}
      <button onclick={onclose}>{t('common.cancel')}</button>
    {/if}
  {/snippet}
</FloatingWindow>

<style>
  progress {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
