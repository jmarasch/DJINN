<script lang="ts">
  // Admin > AI has three tabs: Listen (this), Search (this) and Lyrics (AiWorkPanel).
  let { part }: { part: 'listen' | 'search' } = $props()

  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
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
  /** One decimal, in the language's own way. */
  const dec = (x: number) => x.toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })

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
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
  }

  async function pressed(action: () => Promise<unknown>, done: string) {
    pending = true
    await attempt(action, done)
    pending = false
  }

  async function remove() {
    if (!confirm(t('ai.remove_q'))) return
    await attempt(() => api.removeAi(), t('ai.removed'))
  }

  async function removeReader() {
    if (!confirm(t('ai.remove_reader_q'))) return
    await attempt(() => api.removeReader(), t('ai.reader_removed'))
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
      return { text: t('ai.below'), level: 'bad' }
    }
    if ((memory === null || memory >= spec.goodMemory * 0.95) && system.cores >= spec.goodCores) {
      return { text: t('ai.good'), level: 'good' }
    }
    return { text: t('ai.ok'), level: 'ok' }
  }

  /** A warning when there isn't enough free memory to switch it on now. */
  function tight(spec: Spec, on: boolean): string | null {
    const free = system?.memory_available
    if (on || free == null || free >= spec.uses * 1.5) return null
    return t('ai.tight', { n: dec(free / GB) })
  }
</script>

{#if system}
  <section class="card">
    <h2>{t('ai.computer')}</h2>
    <p class="small">
      {t('ai.cores', { n: system.cores })}{#if system.memory_total !== null}{t('ai.memory', {
          total: gb(system.memory_total),
          free: gb(system.memory_available ?? 0),
        })}{/if}.
    </p>
    <p class="small muted">
      {t('ai.cpu_note')}{reader?.nvidia ? t('ai.nvidia_note') : ''}{t('ai.memory_note')}
    </p>
  </section>
{/if}

{#if part === 'listen'}
<section class="card">
  <h2>{t('ai.listening')}</h2>
  <p class="small muted">
    {t('ai.listening_note')}
  </p>
  {@render specs(LISTENING, '410 MB', t('ai.listen_fast'), t('ai.listen_slow'))}
  {#if ai}
    {@const warn = tight(LISTENING, ai.enabled)}
    {#if warn}<p class="small error">{warn}</p>{/if}
    {#if ai.error}<p class="small error">{ai.error}</p>{/if}
    {#if ai.downloading}
      <p class="small">{t('ai.downloading', { done: mb(ai.downloading[0]), total: mb(ai.downloading[1]) })}</p>
      <progress max={ai.downloading[1]} value={ai.downloading[0]}></progress>
      <button class="small-button" onclick={() => attempt(() => api.cancelAiDownload(), t('ai.download_stopped'))}>
        {t('ai.stop_download')}
      </button>
    {:else if !ai.installed}
      <p class="small muted">{t('ai.clap_note')}</p>
      <button class="primary" onclick={() => attempt(() => api.downloadAi())}>{t('ai.download')}</button>
    {:else}
      <label class="switch">
        <input
          type="checkbox"
          checked={ai.enabled}
          onchange={(e) => attempt(() => api.setAi(e.currentTarget.checked))}
        />
        {t('ai.listen_on')}
      </label>
      {#if ai.enabled}
        {#if !ai.ready}
          <p class="small"><span class="spinner" aria-hidden="true"></span> {t('ai.loading')}</p>
        {:else if ai.waiting > 0}
          <p class="small">
            {#if ai.listening}<span class="spinner" aria-hidden="true"></span>{/if}
            {t('ai.heard', { done: ai.heard, total })}{ai.listening ? '' : t('ai.paused')}
          </p>
          <progress max={total} value={ai.heard}></progress>
          {#if ai.listening}
            <button
              class="small-button"
              disabled={pending}
              onclick={() => pressed(() => api.stopListening(), t('ai.stopping'))}
            >
              {t('ai.stop_now')}
            </button>
          {:else}
            <button
              class="small-button"
              disabled={pending}
              onclick={() => pressed(() => api.continueListening(), t('ai.listening_again'))}
            >
              {pending ? t('ai.starting') : t('ai.continue')}
            </button>
          {/if}
        {:else}
          <p class="small muted">{t('ai.all_heard', { n: total })}</p>
        {/if}
      {/if}
      <div class="options">
        <label>
          {t('ai.speed')}
          <select
            value={ai.options.listen_speed}
            onchange={(e) => setOption('listen_speed', e.currentTarget.value as AiOptions['listen_speed'])}
          >
            <option value="gentle">{t('ai.gentle')}</option>
            <option value="normal">{t('ai.normal')}</option>
            <option value="fast">{t('ai.fast')}</option>
          </select>
          <span class="small muted">{t('ai.next_batch')}</span>
        </label>
        <label class="switch">
          <input
            type="checkbox"
            checked={ai.options.genre_tags}
            onchange={(e) => setOption('genre_tags', e.currentTarget.checked)}
          />
          {t('ai.genre_tags')}
        </label>
      </div>
      <button class="small-button" onclick={remove}>{t('ai.remove')}</button>
    {/if}
  {/if}
</section>
{/if}

{#if part === 'search'}
<section class="card">
  <h2>{t('ai.reader')}</h2>
  <p class="small muted">
    {t('ai.reader_note')}
  </p>
  {@render specs(READER, `${dec(1.1)} GB`, t('ai.reader_fast'), t('ai.reader_slow'))}
  {#if ai && reader}
    {@const warn = tight(READER, reader.enabled)}
    {#if warn}<p class="small error">{warn}</p>{/if}
    {#if reader.error}<p class="small error">{reader.error}</p>{/if}
    {#if reader.downloading}
      <p class="small">{t('ai.downloading', { done: mb(reader.downloading[0]), total: mb(reader.downloading[1]) })}</p>
      <progress max={reader.downloading[1]} value={reader.downloading[0]}></progress>
      <button class="small-button" onclick={() => attempt(() => api.cancelReaderDownload(), t('ai.download_stopped'))}>
        {t('ai.stop_download')}
      </button>
    {:else if !reader.installed}
      <p class="small muted">
        {t('ai.qwen_note')}
      </p>
      <button class="primary" onclick={() => attempt(() => api.downloadReader())}>{t('ai.download')}</button>
    {:else}
      <label class="switch">
        <input
          type="checkbox"
          checked={reader.enabled}
          onchange={(e) => attempt(() => api.setReader(e.currentTarget.checked))}
        />
        {t('ai.reader_on')}
      </label>
      {#if reader.enabled && !reader.ready}
        <p class="small"><span class="spinner" aria-hidden="true"></span> {t('ai.loading')}</p>
      {:else if reader.ready}
        <p class="small muted">{reader.on_gpu ? t('ai.ready_gpu') : t('ai.ready_cpu')}</p>
      {/if}
      {#if reader.nvidia}
        <div class="options">
          <strong class="small">{t('ai.nvidia')}</strong>
          {#if reader.gpu_downloading}
            <p class="small">
              {t('ai.gpu_downloading', { done: mb(reader.gpu_downloading[0]), total: mb(reader.gpu_downloading[1]) })}
            </p>
            <progress max={reader.gpu_downloading[1]} value={reader.gpu_downloading[0]}></progress>
            <button class="small-button" onclick={() => attempt(() => api.cancelGpuPack(), t('ai.download_stopped'))}>
              {t('ai.stop_download')}
            </button>
          {:else if !reader.gpu_installed}
            <p class="small muted">
              {t('ai.gpu_note', { size: mb(reader.gpu_pack_size) })}
            </p>
            <button class="small-button" onclick={() => attempt(() => api.downloadGpuPack())}>{t('ai.gpu_download')}</button>
          {:else}
            <label class="switch">
              <input
                type="checkbox"
                checked={reader.gpu_enabled}
                onchange={(e) => attempt(() => api.setReaderGpu(e.currentTarget.checked))}
              />
              {t('ai.use_gpu')}
            </label>
            {#if reader.gpu_enabled && reader.gpu_note}
              <p class="small error">{t('ai.gpu_fallback', { note: reader.gpu_note })}</p>
            {/if}
            <button class="small-button" onclick={() => attempt(() => api.removeGpuPack(), t('ai.gpu_removed'))}>
              {t('ai.gpu_remove')}
            </button>
          {/if}
          {#if reader.gpu_error}<p class="small error">{reader.gpu_error}</p>{/if}
        </div>
      {/if}
      <div class="options">
        <label>
          {t('ai.read_for')}
          <select
            value={ai.options.reader_for}
            onchange={(e) => setOption('reader_for', e.currentTarget.value as AiOptions['reader_for'])}
          >
            <option value="everyone">{t('ai.everyone')}</option>
            <option value="users">{t('ai.users')}</option>
            <option value="admins">{t('ai.admins')}</option>
          </select>
          <span class="small muted">{t('ai.busy_note')}</span>
        </label>
      </div>
      <button class="small-button" onclick={removeReader}>{t('ai.remove')}</button>
    {/if}
  {/if}
</section>
{/if}

{#snippet specs(spec: Spec, download: string, fast: string, slow: string)}
  {@const fit = verdict(spec)}
  <dl class="specs small">
    <dt>{t('ai.needs')}</dt>
    <dd>{t('ai.needs_line', { gb: dec(spec.uses / GB), download })}</dd>
    <dt>{t('ai.minimum')}</dt>
    <dd>{t('ai.min_line', { memory: gb(spec.minMemory), cores: spec.minCores, speed: slow })}</dd>
    <dt>{t('ai.recommended')}</dt>
    <dd>{t('ai.good_line', { memory: gb(spec.goodMemory), cores: spec.goodCores, speed: fast })}</dd>
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
