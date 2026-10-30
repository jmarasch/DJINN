<script lang="ts">
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
      toast(e instanceof Error ? e.message : 'Could not load', true)
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
            item.error = JSON.parse(xhr.responseText).error ?? `Failed (${xhr.status})`
          } catch {
            item.error = `Failed (${xhr.status})`
          }
        }
        resolve()
      }
      xhr.onerror = () => {
        item.error = 'The connection dropped'
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
      toast(e instanceof Error ? e.message : 'Could not take it back', true)
    }
  }

  const status = {
    waiting: 'Waiting for review',
    approved: 'In the library',
    declined: 'Turned down',
  }
  const when = (ms: number) => new Date(ms).toLocaleDateString([], { month: 'short', day: 'numeric' })
</script>

<h1>Submit music</h1>
{#if !app.me?.permissions?.submit_music}
  <p class="muted">Your account can't submit music. Ask the host if you'd like to.</p>
{:else}
  <p class="muted">
    Send song files for the library: MP3, FLAC, M4A (AAC), Ogg Vorbis or WAV.
    {app.me.permissions.submit_without_review
      ? 'They go straight in, unless they look like a song the library has already.'
      : 'The host reviews each one first.'}
    Good tags (title, artist, album) help them land in the right place.
  </p>
  <label class="button primary pick">
    Choose songs
    <input bind:this={input} type="file" accept=".mp3,.flac,.m4a,.mp4,.aac,.ogg,.oga,.wav" multiple onchange={chosen} hidden />
  </label>

  {#each sending as item, i (i)}
    <div class="sending">
      <span class="grow">{item.name}</span>
      {#if item.error}
        <span class="error small">{item.error}</span>
      {:else if item.done}
        <span class="muted small">Sent</span>
      {:else}
        <progress max="1" value={item.progress}></progress>
      {/if}
    </div>
  {/each}

  <h2>Sent so far</h2>
  {#each mine as s (s.id)}
    <div class="item">
      <div class="grow">
        <div>{s.title ?? s.file_name} <span class="muted small">{s.artist ?? ''}</span></div>
        <div class="muted small">
          {status[s.status]}{#if s.note}: {s.note}{/if} · {when(s.created_at)}{#if s.duration_ms} · {duration(s.duration_ms)}{/if}
        </div>
        {#if s.status === 'waiting' && s.duplicate_title}
          <div class="muted small">Looks like {s.duplicate_title}, already in the library.</div>
        {/if}
      </div>
      {#if s.status === 'waiting'}<button class="small-button" onclick={() => withdraw(s)}>Take back</button>{/if}
    </div>
  {:else}
    <p class="muted">Nothing yet.</p>
  {/each}
{/if}

<style>
  h1 {
    margin: 0 0 4px;
  }

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
