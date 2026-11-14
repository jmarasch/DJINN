<script lang="ts">
  // Admin > AI > Lyrics listening: a local AI listens to songs that have no
  // lyrics and makes them (or times plain ones). Everything runs on this
  // computer or others you set up; models are downloaded once and nothing
  // is sent anywhere but to your own DJINN.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { AiListenerStatus, AiWorkStatus } from './types'

  let work = $state<AiWorkStatus | null>(null)
  let listener = $state<AiListenerStatus | null>(null)
  let newKeyName = $state('')
  let madeKey = $state<{ name: string; key: string } | null>(null)

  const n = (x: number) => x.toLocaleString()
  const models = ['tiny', 'base', 'small', 'medium', 'large-v3-turbo']

  async function refresh() {
    try {
      work = await api.aiWork()
      listener = await api.aiListener()
    } catch {
      // Shown next time.
    }
  }

  onMount(() => {
    void refresh()
    const timer = setInterval(() => void refresh(), 3000)
    return () => clearInterval(timer)
  })

  async function saveWork() {
    if (!work) return
    try {
      await api.setAiWork({ on: work.on, new_songs: work.new_songs, sync_plain: work.sync_plain })
      if (work.on && listener?.installed && listener.auto_start && !listener.running) await api.startAiListener()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
    void refresh()
  }

  async function saveListener() {
    if (!listener) return
    try {
      await api.setAiListener({ model: listener.model, vocals: listener.vocals, auto_start: listener.auto_start })
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
  }

  async function act(run: () => Promise<unknown>, done?: string) {
    try {
      await run()
      if (done) toast(done)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
    void refresh()
  }

  async function makeKey() {
    if (!newKeyName.trim()) return
    try {
      madeKey = await api.makeAiWorkerKey(newKeyName.trim())
      newKeyName = ''
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not make a key', true)
    }
    void refresh()
  }

  const held = $derived(
    work?.held
      ? work.held.reason === 'music'
        ? 'music is playing'
        : `it is outside the hours you set (${work.held.from}:00 to ${work.held.until}:00)`
      : '',
  )
</script>

<section class="card">
  <h3>Lyrics listening</h3>
  <p class="small muted">
    A local AI listens to songs that have no lyrics and writes them with timing, or times lyrics you already have. It
    runs on this computer (or others you set up) with models downloaded once; nothing leaves your network. Its lyrics are
    marked as AI-made, rank below every other source, and show under Library > Problems.
  </p>
  {#if work}
    <label class="switch">
      <input type="checkbox" bind:checked={work.on} onchange={saveWork} />
      Let the listener work
    </label>
    <p class="small muted sub">
      It only works when background work is allowed (the hours or "no music playing" rule under Library) and stops
      between songs when it isn't.
      {#if work.on && held}<strong>Paused now: {held}.</strong>{/if}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={work.new_songs} onchange={saveWork} />
      Songs with no lyrics (after LRCLIB has been asked)
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={work.sync_plain} onchange={saveWork} />
      Time plain lyrics (a person's words, timed by the AI)
    </label>
    <p class="small">
      Waiting: {n(work.counts.waiting_listen)} without lyrics, {n(work.counts.waiting_sync)} plain.
      Working: {n(work.counts.working)}. Done: {n(work.counts.done)}. Failed: {n(work.counts.failed)}
      {#if work.counts.failed}<button class="link" onclick={() => act(api.retryAiWork, 'Will try again')}>Try again</button>{/if}
    </p>
  {/if}

  <h4>This computer</h4>
  {#if listener}
    <p class="small">
      Python: {listener.python ?? 'not found'} · ffmpeg: {listener.ffmpeg ? 'found' : 'not found'} · Graphics card:
      {listener.nvidia ? 'NVIDIA found' : 'none (it will use the processor)'}
    </p>
    <label>
      Model
      <select bind:value={listener.model} onchange={saveListener} disabled={listener.installing}>
        {#each models as m (m)}<option value={m}>{m}</option>{/each}
      </select>
    </label>
    <p class="small muted sub">
      Bigger is more accurate and slower: tiny and base suit a small processor, small a decent one, medium or
      large-v3-turbo a graphics card.
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={listener.vocals} onchange={saveListener} disabled={listener.installing} />
      Isolate the vocals first (much bigger download, slower, helps on loud mixes)
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={listener.auto_start} onchange={saveListener} />
      Start it with DJINN
    </label>
    <div class="actions">
      <button onclick={() => act(api.installAiListener)} disabled={listener.installing}>
        {listener.installed ? 'Install again (after changing the model)' : 'Install the listener'}
      </button>
      {#if listener.installed}
        {#if listener.running}
          <button onclick={() => act(api.stopAiListener, 'Stopped')}>Stop</button>
        {:else}
          <button class="primary" onclick={() => act(api.startAiListener, 'Started')}>Start</button>
        {/if}
      {/if}
      <span class="small">{listener.installing ? 'Installing...' : listener.running ? 'Running' : listener.installed ? 'Installed, not running' : 'Not installed'}</span>
    </div>
    {#if listener.error}<p class="small bad">{listener.error}</p>{/if}
    {#if listener.log.length}<pre class="log small">{listener.log.join('\n')}</pre>{/if}
  {/if}

  <h4>Other computers</h4>
  <p class="small muted">
    A computer with a decent processor or graphics card can listen too. Make a key for it, copy the folder
    <code>ai-listener</code> from DJINN's data folder to it, and run
    <code>python worker.py --server http://this-computer:8066 --key KEY</code>.
  </p>
  {#if work}
    <ul class="keys">
      {#each work.keys as name (name)}
        <li>
          <strong>{name}</strong>
          {#each work.workers.filter((w) => w.name === name) as w (w.name)}
            <span class="small muted">seen {new Date(w.last_seen).toLocaleTimeString()} · {n(w.done)} done · {n(w.failed)} failed</span>
          {/each}
          <button class="small-button" onclick={() => act(() => api.removeAiWorkerKey(name))}>Remove</button>
        </li>
      {/each}
    </ul>
    <div class="actions">
      <input placeholder="Name of the computer" bind:value={newKeyName} />
      <button onclick={makeKey} disabled={!newKeyName.trim()}>Make a key</button>
    </div>
    {#if madeKey}
      <p class="small">
        Key for {madeKey.name} (shown once): <code>{madeKey.key}</code>
      </p>
    {/if}
  {/if}
</section>

<style>
  .sub {
    margin: 2px 0 10px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin: 8px 0;
  }

  .log {
    max-height: 200px;
    overflow: auto;
    white-space: pre-wrap;
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 8px;
  }

  .keys {
    list-style: none;
    margin: 8px 0;
    padding: 0;
  }

  .keys li {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 4px 0;
  }

  .bad {
    color: var(--danger, #d33);
  }
</style>
