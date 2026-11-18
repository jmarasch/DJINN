<script lang="ts">
  import { locale, t } from '../../lib/i18n.svelte'
  import PageTitle from '../../lib/PageTitle.svelte'
  // Sending songs for the library, and what happened to them.
  import { onMount } from 'svelte'
  import { api } from '../../lib/api'
  import { duration } from '../../lib/format'
  import { app, toast } from '../../lib/state.svelte'
  import type { Submission } from '../../lib/types'

  type Sending = { name: string; progress: number; error: string; done: boolean }

  let mine = $state<Submission[]>([])
  let sending = $state<Sending[]>([])
  let input = $state<HTMLInputElement | undefined>()

  onMount(load)

  async function load() {
    try {
      mine = await api.mySubmissions()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.load_failed'), true)
    }
  }

  /** Sends one file, reporting progress (fetch can't, so XHR). */
  function send(file: File, item: Sending): Promise<void> {
    return new Promise((resolve) => {
      const xhr = new XMLHttpRequest()
      xhr.open('POST', `/api/go/submissions?name=${encodeURIComponent(file.name)}`)
      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable) item.progress = e.loaded / e.total
      }
      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          item.done = true
        } else {
          try {
            item.error = JSON.parse(xhr.responseText).error ?? t('gs.failed_status', { status: xhr.status })
          } catch {
            item.error = t('gs.failed_status', { status: xhr.status })
          }
        }
        resolve()
      }
      xhr.onerror = () => {
        item.error = t('gs.dropped')
        resolve()
      }
      xhr.send(file)
    })
  }

  async function chosen() {
    const files = Array.from(input?.files ?? [])
    if (!files.length) return
    const start = sending.length
    sending.push(...files.map((f) => ({ name: f.name, progress: 0, error: '', done: false })))
    // One at a time, so a slow connection isn't split many ways.
    for (const [i, file] of files.entries()) await send(file, sending[start + i])
    if (input) input.value = ''
    await load()
  }

  async function withdraw(s: Submission) {
    try {
      await api.withdrawSubmission(s.id)
      mine = mine.filter((m) => m.id !== s.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gs.take_back_failed'), true)
    }
  }

  const status = $derived({
    waiting: t('si.waiting'),
    approved: t('pa.k_approved_submission'),
    declined: t('sub.decline'),
  })
  const when = (ms: number) => new Date(ms).toLocaleDateString(locale.code, { month: 'short', day: 'numeric' })
</script>

<PageTitle title={t('gs.title')} />
{#if !app.me?.permissions?.submit_music}
  <p class="muted">{t('gs.cant')}</p>
{:else}
  <p class="muted">
    {t('gs.intro')}
    {app.me.permissions.submit_without_review ? t('gs.straight') : t('gs.review')}
    {t('gs.tags')}
  </p>
  <label class="button primary pick">
    {t('gs.choose')}
    <input bind:this={input} type="file" accept=".mp3,.flac,.m4a,.mp4,.aac,.ogg,.oga,.wav" multiple onchange={chosen} hidden />
  </label>

  {#each sending as item, i (i)}
    <div class="sending">
      <span class="grow">{item.name}</span>
      {#if item.error}
        <span class="error small">{item.error}</span>
      {:else if item.done}
        <span class="muted small">{t('gs.sent')}</span>
      {:else}
        <progress max="1" value={item.progress}></progress>
      {/if}
    </div>
  {/each}

  <h2>{t('gs.so_far')}</h2>
  {#each mine as s (s.id)}
    <div class="item">
      <div class="grow">
        <div>{s.title ?? s.file_name} <span class="muted small">{s.artist ?? ''}</span></div>
        <div class="muted small">
          {status[s.status]}{#if s.note}: {s.note}{/if} · {when(s.created_at)}{#if s.duration_ms} · {duration(s.duration_ms)}{/if}
        </div>
        {#if s.status === 'waiting' && s.duplicate_title}
          <div class="muted small">{t('sub.dup', { title: s.duplicate_title })}</div>
        {/if}
      </div>
      {#if s.status === 'waiting'}<button class="small-button" onclick={() => withdraw(s)}>{t('gs.take_back')}</button>{/if}
    </div>
  {:else}
    <p class="muted">{t('gs.nothing')}</p>
  {/each}
{/if}

<style>

  h2 {
    margin: 20px 0 8px;
    font-size: 1.05rem;
  }

  .pick {
    display: inline-flex;
    margin: 8px 0;
    cursor: pointer;
  }

  .sending,
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  progress {
    width: 120px;
  }
</style>
