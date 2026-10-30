<script lang="ts">
  // Admin > GO: music people sent from DJINN GO, waiting for review, and
  // where approved songs go.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { duration } from './format'
  import { toast } from './state.svelte'
  import type { Submission, UploadSettings } from './types'

  let waiting = $state<Submission[]>([])
  let settings = $state<UploadSettings | null>(null)
  let busy = $state(false)

  onMount(() => {
    load().catch((e) => toast(e.message, true))
  })

  async function load() {
    ;[waiting, settings] = await Promise.all([api.waitingSubmissions(), api.uploadSettings()])
  }

  async function approve(s: Submission) {
    busy = true
    try {
      await api.approveSubmission(s.id)
      waiting = waiting.filter((w) => w.id !== s.id)
      toast(`Added "${s.title ?? s.file_name}" to the library`)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add it', true)
    } finally {
      busy = false
    }
  }

  async function decline(s: Submission) {
    const note = prompt(`Turn down "${s.title ?? s.file_name}"? The file is deleted. A note for ${s.uploader ?? 'them'} (optional):`, '')
    if (note === null) return
    try {
      await api.declineSubmission(s.id, note)
      waiting = waiting.filter((w) => w.id !== s.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not turn it down', true)
    }
  }

  async function saveSettings(e: SubmitEvent) {
    e.preventDefault()
    if (!settings) return
    try {
      settings = await api.saveUploadSettings(settings)
      toast('Saved')
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not save', true)
    }
  }

  const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`
</script>

<section class="card">
  <h2>Submitted music</h2>
  <p class="small muted">
    Songs people sent from DJINN GO, for roles that can submit music. Listen, then add them to the library or turn them down.
  </p>
  {#each waiting as s (s.id)}
    <div class="item">
      <div>
        <b>{s.title ?? s.file_name}</b>
        <span class="muted">{[s.artist, s.album].filter(Boolean).join(' · ')}</span>
      </div>
      <div class="muted small">
        From {s.uploader ?? 'someone'} · {s.file_name} · {mb(s.size)}{#if s.duration_ms} · {duration(s.duration_ms)}{/if}
      </div>
      {#if s.duplicate_title}<div class="warn small">Looks like {s.duplicate_title}, already in the library.</div>{/if}
      {#if !s.title || !s.artist}<div class="warn small">Missing {!s.title ? 'a title' : 'an artist'} in its tags.</div>{/if}
      <audio controls preload="none" src="/api/submissions/{s.id}/file"></audio>
      <div class="buttons">
        <button class="primary small-button" onclick={() => approve(s)} disabled={busy}>Add to library</button>
        <button class="small-button" onclick={() => decline(s)} disabled={busy}>Turn down</button>
      </div>
    </div>
  {:else}
    <p class="muted">Nothing waiting.</p>
  {/each}

  {#if settings}
    <form class="settings" onsubmit={saveSettings}>
      <label>
        Approved songs go to
        <input bind:value={settings.folder} spellcheck="false" />
      </label>
      <p class="small muted">
        A folder just for uploads, sorted into Artist/Album folders. It becomes a library folder the first time a song goes in. DJINN never writes to your other music folders.
      </p>
      <label>
        Largest file (MB)
        <input type="number" min="1" max="4096" bind:value={settings.max_mb} />
      </label>
      <button class="small-button">Save</button>
    </form>
  {/if}
</section>

<style>
  .item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .warn {
    color: var(--bad);
  }

  audio {
    width: 100%;
  }

  .buttons {
    display: flex;
    gap: 8px;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
  }

  .settings p {
    margin: 0;
  }

  .settings input[type='number'] {
    width: 120px;
  }

  .settings button {
    align-self: flex-start;
  }
</style>
