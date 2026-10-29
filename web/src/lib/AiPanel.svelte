<script lang="ts">
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { AiOptions } from './types'

  $effect(() => {
    api
      .ai()
      .then((s) => (app.ai = s))
      .catch((e) => toast(e.message, true))
  })

  const GB = 2 ** 30
  const mb = (bytes: number) => `${Math.round(bytes / 1_000_000)} MB`
  const gb = (bytes: number) => `${Math.round(bytes / GB)} GB`

  /** What each model needs, measured on a real library. */
  interface Spec {
    /** Memory while on. */
    uses: number
    minMemory: number
    minCores: number
    goodMemory: number
    goodCores: number
  }
  const LISTENING: Spec = { uses: 0.7 * GB, minMemory: 4 * GB, minCores: 2, goodMemory: 8 * GB, goodCores: 4 }
  const READER: Spec = { uses: 1.6 * GB, minMemory: 8 * GB, minCores: 4, goodMemory: 16 * GB, goodCores: 8 }

  /** A stop or continue button was pressed and hasn't been answered yet. */
  let pending = $state(false)

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  async function pressed(action: () => Promise<unknown>, done: string) {
    pending = true
    await attempt(action, done)
    pending = false
  }

  async function remove() {
    if (!confirm('Remove the AI model? Songs it already heard are forgotten too until you download it again.')) return
    await attempt(() => api.removeAi(), 'AI model removed')
  }

  async function removeReader() {
    if (!confirm('Remove the AI reader model?')) return
    await attempt(() => api.removeReader(), 'AI reader removed')
  }

  function setOption<K extends keyof AiOptions>(key: K, value: AiOptions[K]) {
    if (!ai) return
    attempt(() => api.setAiOptions({ ...ai.options, [key]: value }))
  }

  const ai = $derived(app.ai)
  const reader = $derived(app.ai?.reader)
  const total = $derived(ai ? ai.heard + ai.waiting : 0)
  const system = $derived(ai?.system)

  /** How this computer compares with `spec`. */
  function verdict(spec: Spec): { text: string; level: 'good' | 'ok' | 'bad' } | null {
    if (!system) return null
    const memory = system.memory_total
    if ((memory !== null && memory < spec.minMemory * 0.95) || system.cores < spec.minCores) {
      return { text: 'This computer is below the minimum. It may be very slow or slow down the music.', level: 'bad' }
    }
    if ((memory === null || memory >= spec.goodMemory * 0.95) && system.cores >= spec.goodCores) {
      return { text: 'This computer meets the recommended specs.', level: 'good' }
    }
    return { text: 'This computer meets the minimum; expect it to be slower.', level: 'ok' }
  }

  /** A warning when there isn't enough free memory to switch it on now. */
  function tight(spec: Spec, on: boolean): string | null {
    const free = system?.memory_available
    if (on || free == null || free >= spec.uses * 1.5) return null
    return `Only ${(free / GB).toFixed(1)} GB of memory is free right now. Close other programs before switching it on.`
  }
</script>

{#if system}
  <section class="card">
    <h2>This computer</h2>
    <p class="small">
      {system.cores} processor {system.cores === 1 ? 'core' : 'cores'}{#if system.memory_total !== null}, {gb(
          system.memory_total,
        )} of memory ({gb(system.memory_available ?? 0)} free right now){/if}.
    </p>
    <p class="small muted">
      The AI tools are optional and run on the processor (CPU), so no graphics card is needed.{reader?.nvidia
        ? ' This computer has an NVIDIA graphics card, which can make the AI reader much faster (see below).'
        : ''} Without enough memory they can slow the whole computer down, music included, so check the specs below.
    </p>
  </section>
{/if}

<section class="card">
  <h2>AI listening (optional)</h2>
  <p class="small muted">
    An AI model listens to each song once, so searching by description ("80s hair metal", "music for cooking dinner")
    matches how songs actually sound, even ones with no genre tags. New songs are listened to as they're added. It runs
    on this computer; nothing is sent anywhere, and your music folders aren't changed.
  </p>
  {@render specs(LISTENING, '410 MB', 'about 1 second per song in the background', 'a few seconds per song')}
  {#if ai}
    {@const warn = tight(LISTENING, ai.enabled)}
    {#if warn}<p class="small error">{warn}</p>{/if}
    {#if ai.error}<p class="small error">{ai.error}</p>{/if}
    {#if ai.downloading}
      <p class="small">Downloading the model: {mb(ai.downloading[0])} of {mb(ai.downloading[1])}</p>
      <progress max={ai.downloading[1]} value={ai.downloading[0]}></progress>
      <button class="small-button" onclick={() => attempt(() => api.cancelAiDownload(), 'Download stopped')}>
        Stop download
      </button>
    {:else if !ai.installed}
      <p class="small muted">The model (LAION's CLAP for music, free to use) is a one-time download from Hugging Face.</p>
      <button class="primary" onclick={() => attempt(() => api.downloadAi())}>Download the model</button>
    {:else}
      <label class="switch">
        <input
          type="checkbox"
          checked={ai.enabled}
          onchange={(e) => attempt(() => api.setAi(e.currentTarget.checked))}
        />
        Listen to songs with AI
      </label>
      {#if ai.enabled}
        {#if !ai.ready}
          <p class="small"><span class="spinner" aria-hidden="true"></span> Loading the model...</p>
        {:else if ai.waiting > 0}
          <p class="small">
            {#if ai.listening}<span class="spinner" aria-hidden="true"></span>{/if}
            Listened to {ai.heard} of {total} songs.{ai.listening
              ? ''
              : ' Stopped for now; it carries on when DJINN restarts or new music arrives.'}
          </p>
          <progress max={total} value={ai.heard}></progress>
          {#if ai.listening}
            <button
              class="small-button"
              disabled={pending}
              onclick={() => pressed(() => api.stopListening(), 'Stopping after these songs')}
            >
              Stop for now
            </button>
          {:else}
            <button
              class="small-button"
              disabled={pending}
              onclick={() => pressed(() => api.continueListening(), 'Listening again')}
            >
              {pending ? 'Starting...' : 'Continue'}
            </button>
          {/if}
        {:else}
          <p class="small muted">Listened to all {total} songs.</p>
        {/if}
      {/if}
      <div class="options">
        <label>
          Listening speed
          <select
            value={ai.options.listen_speed}
            onchange={(e) => setOption('listen_speed', e.currentTarget.value as AiOptions['listen_speed'])}
          >
            <option value="gentle">Gentle: one song at a time, lightest on the computer</option>
            <option value="normal">Normal</option>
            <option value="fast">Fast: uses half the processor</option>
          </select>
          <span class="small muted">Takes effect from the next batch of songs.</span>
        </label>
        <label class="switch">
          <input
            type="checkbox"
            checked={ai.options.genre_tags}
            onchange={(e) => setOption('genre_tags', e.currentTarget.checked)}
          />
          Give songs with no genres the ones the AI hears
        </label>
      </div>
      <button class="small-button" onclick={remove}>Remove the model</button>
    {/if}
  {/if}
</section>

<section class="card">
  <h2>AI reader (optional)</h2>
  <p class="small muted">
    A small language model reads searches that DJINN's own phrase reader can't place, like "something for a first
    dance at a wedding, no rap", and turns them into genres, years, and moods. It runs on this computer; nothing is
    sent anywhere. Works best with AI listening on.
  </p>
  {@render specs(READER, '1.1 GB', '2 to 4 seconds per new search', 'about 10 seconds per new search')}
  {#if ai && reader}
    {@const warn = tight(READER, reader.enabled)}
    {#if warn}<p class="small error">{warn}</p>{/if}
    {#if reader.error}<p class="small error">{reader.error}</p>{/if}
    {#if reader.downloading}
      <p class="small">Downloading the model: {mb(reader.downloading[0])} of {mb(reader.downloading[1])}</p>
      <progress max={reader.downloading[1]} value={reader.downloading[0]}></progress>
      <button class="small-button" onclick={() => attempt(() => api.cancelReaderDownload(), 'Download stopped')}>
        Stop download
      </button>
    {:else if !reader.installed}
      <p class="small muted">
        The model (Qwen2.5 1.5B Instruct, free to use) is a one-time download from Hugging Face. Searches it has read
        before are instant.
      </p>
      <button class="primary" onclick={() => attempt(() => api.downloadReader())}>Download the model</button>
    {:else}
      <label class="switch">
        <input
          type="checkbox"
          checked={reader.enabled}
          onchange={(e) => attempt(() => api.setReader(e.currentTarget.checked))}
        />
        Read searches with AI
      </label>
      {#if reader.enabled && !reader.ready}
        <p class="small"><span class="spinner" aria-hidden="true"></span> Loading the model...</p>
      {:else if reader.ready}
        <p class="small muted">Ready, running on the {reader.on_gpu ? 'graphics card (GPU)' : 'processor (CPU)'}.</p>
      {/if}
      {#if reader.nvidia}
        <div class="options">
          <strong class="small">NVIDIA graphics card</strong>
          {#if reader.gpu_downloading}
            <p class="small">
              Downloading the GPU pack: {mb(reader.gpu_downloading[0])} of {mb(reader.gpu_downloading[1])}
            </p>
            <progress max={reader.gpu_downloading[1]} value={reader.gpu_downloading[0]}></progress>
            <button class="small-button" onclick={() => attempt(() => api.cancelGpuPack(), 'Download stopped')}>
              Stop download
            </button>
          {:else if !reader.gpu_installed}
            <p class="small muted">
              On the graphics card each new search takes about 0.3 seconds instead of 2 to 10. That needs DJINN's GPU
              pack: about {mb(reader.gpu_pack_size)} from GitHub, with NVIDIA's CUDA libraries, so nothing else has to
              be installed. If the card ever fails, DJINN goes back to the processor by itself.
            </p>
            <button class="small-button" onclick={() => attempt(() => api.downloadGpuPack())}>Download the GPU pack</button>
          {:else}
            <label class="switch">
              <input
                type="checkbox"
                checked={reader.gpu_enabled}
                onchange={(e) => attempt(() => api.setReaderGpu(e.currentTarget.checked))}
              />
              Use the graphics card
            </label>
            {#if reader.gpu_enabled && reader.gpu_note}
              <p class="small error">On the processor instead: {reader.gpu_note}. Switch it off and on to try again.</p>
            {/if}
            <button class="small-button" onclick={() => attempt(() => api.removeGpuPack(), 'GPU pack removed')}>
              Remove the GPU pack
            </button>
          {/if}
          {#if reader.gpu_error}<p class="small error">{reader.gpu_error}</p>{/if}
        </div>
      {/if}
      <div class="options">
        <label>
          Read searches for
          <select
            value={ai.options.reader_for}
            onchange={(e) => setOption('reader_for', e.currentTarget.value as AiOptions['reader_for'])}
          >
            <option value="everyone">Everyone, guests too</option>
            <option value="users">Signed-in users, not guests</option>
            <option value="admins">Admins only</option>
          </select>
          <span class="small muted">Each new search keeps the processor busy for a few seconds.</span>
        </label>
      </div>
      <button class="small-button" onclick={removeReader}>Remove the model</button>
    {/if}
  {/if}
</section>

{#snippet specs(spec: Spec, download: string, fast: string, slow: string)}
  {@const fit = verdict(spec)}
  <dl class="specs small">
    <dt>Needs</dt>
    <dd>about {(spec.uses / GB).toFixed(1)} GB of memory while on, {download} of disk</dd>
    <dt>Minimum</dt>
    <dd>{gb(spec.minMemory)} memory, {spec.minCores} cores ({slow})</dd>
    <dt>Recommended</dt>
    <dd>{gb(spec.goodMemory)} memory, {spec.goodCores} or more cores ({fast})</dd>
  </dl>
  {#if fit}<p class="small fit {fit.level}">{fit.text}</p>{/if}
{/snippet}

<style>
  progress {
    width: 100%;
    accent-color: var(--accent);
  }

  .error,
  .fit.bad {
    color: var(--bad);
  }

  .fit.ok {
    color: var(--muted);
  }

  .fit.good {
    color: var(--accent);
  }

  .specs {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 12px;
    margin: 0;
  }

  .specs dt {
    color: var(--muted);
  }

  .specs dd {
    margin: 0;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--line);
  }

  .small-button {
    align-self: flex-start;
  }
</style>
