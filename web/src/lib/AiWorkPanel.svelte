<script lang="ts">
  // Admin > AI > Lyrics listening: a local AI listens to songs that have no
  // lyrics and makes them (or times plain ones). Everything runs on this
  // computer or others you set up; models are downloaded once and nothing
  // is sent anywhere but to your own DJINN.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { AiHelper, AiListenerStatus, AiWorkStatus } from './types'
  import { aiWhy } from './nodeInfo'

  let work = $state<AiWorkStatus | null>(null)
  let listener = $state<AiListenerStatus | null>(null)
  let newKeyName = $state('')
  let madeKey = $state<{ name: string; key: string } | null>(null)
  // The minimum score, kept apart from `work` so the 3-second refresh doesn't
  // move the slider under a finger.
  let minMatch = $state<number | null>(null)
  // Genres left out of AI lyrics, and every genre in the library to pick from.
  let skipGenres = $state<string[] | null>(null)
  let known = $state<string[]>([])
  let genreText = $state('')

  const n = (x: number) => x.toLocaleString(locale.code)

  /** A node that hasn't checked in for this long isn't connected. */
  const QUIET_MS = 30_000

  type Light = 'grey' | 'red' | 'yellow' | 'green'

  /** A node's light and what it's doing, in words: green working, yellow
   * waiting for a song, red not available now (playing, a problem), grey
   * not able to help. */
  function helperState(h: AiHelper, workOn: boolean): { light: Light; text: string } {
    const song = h.working_on[0]
    const name = song ? (song.artist ? `${song.artist} - ${song.title}` : song.title) : ''
    const phase = h.report.phase
    if (h.last_seen === null || Date.now() - h.last_seen > QUIET_MS) {
      return { light: 'grey', text: t('aw.h_quiet') }
    }
    if (phase === 'failed') {
      return { light: 'red', text: t('aw.h_failed', { why: h.report.detail ?? '' }) }
    }
    if (phase === 'paused') {
      const why = aiWhy(h.report.detail) ?? ''
      return { light: 'red', text: song ? t('aw.h_paused_holding', { why, song: name }) : t('aw.h_paused', { why }) }
    }
    if (phase === 'installing') return { light: 'grey', text: t('aw.h_installing') }
    if (phase === 'off' || phase === '') return { light: 'grey', text: t('aw.h_off') }
    if (!workOn) return { light: 'grey', text: t('aw.h_work_off') }
    if (song) {
      return { light: 'green', text: song.kind === 'sync' ? t('aw.h_timing', { song: name }) : t('aw.h_listening', { song: name }) }
    }
    if (h.answer === 'no_songs') return { light: 'yellow', text: t('aw.h_no_songs') }
    if (h.answer === 'waiting_vocals') return { light: 'yellow', text: t('aw.h_waiting_vocals') }
    return { light: 'yellow', text: t('aw.h_waiting') }
  }
  const models = ['tiny', 'base', 'small', 'medium', 'large-v3-turbo']

  async function refresh() {
    try {
      work = await api.aiWork()
      if (minMatch === null) minMatch = work.min_match
      if (skipGenres === null) skipGenres = [...work.skip_genres]
      listener = await api.aiListener()
    } catch {
      // Shown next time.
    }
  }

  onMount(() => {
    api
      .genres()
      .then((map) => (known = map.genres.map((g) => g.name).sort()))
      .catch(() => {})
    void refresh()
    const timer = setInterval(() => void refresh(), 3000)
    return () => clearInterval(timer)
  })

  async function saveWork() {
    if (!work) return
    try {
      await api.setAiWork({
        on: work.on,
        new_songs: work.new_songs,
        sync_plain: work.sync_plain,
        vocals_for_nodes: work.vocals_for_nodes,
        min_match: minMatch ?? work.min_match,
        skip_genres: skipGenres ?? work.skip_genres,
      })
      if (work.on && listener?.installed && listener.auto_start && !listener.running) await api.startAiListener()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
    void refresh()
  }

  function addGenre() {
    const genre = genreText.trim().toLowerCase()
    genreText = ''
    if (!genre || !skipGenres || skipGenres.includes(genre)) return
    skipGenres = [...skipGenres, genre]
    void saveWork()
  }

  function removeGenre(genre: string) {
    if (!skipGenres) return
    skipGenres = skipGenres.filter((g) => g !== genre)
    void saveWork()
  }

  async function saveListener() {
    if (!listener) return
    try {
      await api.setAiListener({
        model: listener.model,
        vocals: listener.vocals,
        auto_start: listener.auto_start,
        listen_on: listener.listen_on,
      })
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
  }

  async function act(run: () => Promise<unknown>, done?: string) {
    try {
      await run()
      if (done) toast(done)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
    void refresh()
  }

  async function makeKey() {
    if (!newKeyName.trim()) return
    try {
      madeKey = await api.makeAiWorkerKey(newKeyName.trim())
      newKeyName = ''
    } catch (e) {
      toast(e instanceof Error ? e.message : t('aw.key_failed'), true)
    }
    void refresh()
  }

  const held = $derived(
    work?.held
      ? work.held.reason === 'music'
        ? t('aw.music')
        : t('aw.hours', { from: work.held.from, until: work.held.until })
      : '',
  )
</script>

<section class="card">
  <h3>{t('aw.title')}</h3>
  <p class="small muted">
    {t('aw.intro')}
  </p>
  {#if work}
    <label class="switch">
      <input type="checkbox" bind:checked={work.on} onchange={saveWork} />
      {t('aw.on')}
    </label>
    <p class="small muted sub">
      {t('aw.on_note')}
      {#if work.on && held}<strong>{t('aw.paused', { why: held })}</strong>{/if}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={work.new_songs} onchange={saveWork} />
      {t('aw.new_songs')}
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={work.sync_plain} onchange={saveWork} />
      {t('aw.sync_plain')}
    </label>
    <label class="range">
      <span>{t('aw.min', { pct: minMatch ?? work.min_match })}</span>
      <input type="range" min="0" max="100" step="1" bind:value={minMatch} onchange={saveWork} />
    </label>
    <p class="small muted sub">
      {t('aw.min_note')}
    </p>
    <h4>{t('aw.skip')}</h4>
    <p class="small muted sub">
      {t('aw.skip_note')}
    </p>
    {#if skipGenres?.length}
      <div class="chips">
        {#each skipGenres as genre (genre)}
          <span class="chip">
            {genre}
            <button class="icon-only" aria-label={t('aw.unskip', { genre })} onclick={() => removeGenre(genre)}>×</button>
          </span>
        {/each}
      </div>
    {/if}
    <form
      class="add-genre"
      onsubmit={(e) => {
        e.preventDefault()
        addGenre()
      }}
    >
      <input list="ai-skip-genres" bind:value={genreText} placeholder={t('aw.genre_hint')} aria-label={t('aw.genre_label')} />
      <datalist id="ai-skip-genres">
        {#each known as g (g)}<option value={g}></option>{/each}
      </datalist>
      <button class="small-button" type="submit" disabled={!genreText.trim()}>{t('aw.leave_out')}</button>
    </form>
    <p class="small">
      {t('aw.waiting', { listen: n(work.counts.waiting_listen), sync: n(work.counts.waiting_sync) })}
      {t('aw.working', { n: n(work.counts.working) })}{#if work.counts.prepared}
        {t('aw.prepared', { n: n(work.counts.prepared) })}{/if}
      {t('aw.done_failed', { done: n(work.counts.done), failed: n(work.counts.failed) })}
      {#if work.counts.failed}<button class="link" onclick={() => act(api.retryAiWork, t('aw.retry'))}
          >{t('common.try_again')}</button
        >{/if}
    </p>
  {/if}

  <h4>{t('aw.computer')}</h4>
  {#if listener}
    <p class="small">
      {t('aw.facts', {
        python: listener.python ?? t('aw.not_found'),
        ffmpeg: listener.ffmpeg ? t('aw.found') : t('aw.not_found'),
        gpu: listener.nvidia ? t('aw.nvidia') : t('aw.no_gpu'),
      })}
    </p>
    <label>
      {t('aw.model')}
      <select bind:value={listener.model} onchange={saveListener} disabled={listener.installing}>
        {#each models as m (m)}<option value={m}>{m}</option>{/each}
      </select>
    </label>
    <p class="small muted sub">
      {t('aw.model_note')}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={listener.vocals} onchange={saveListener} disabled={listener.installing} />
      {t('aw.vocals')}
    </label>
    {#if listener.vocals && listener.nvidia}
      <label class="sub-choice">
        {t('aw.listen_on')}
        <select bind:value={listener.listen_on} onchange={saveListener} disabled={listener.installing}>
          <option value="gpu">{t('aw.listen_on_gpu')}</option>
          <option value="cpu">{t('aw.listen_on_cpu')}</option>
        </select>
      </label>
      <p class="small muted sub">{t('aw.listen_on_note')}</p>
    {/if}
    <label class="switch">
      <input type="checkbox" bind:checked={listener.auto_start} onchange={saveListener} />
      {t('aw.auto_start')}
    </label>
    {#if work}
      <label class="switch">
        <input
          type="checkbox"
          bind:checked={work.vocals_for_nodes}
          onchange={saveWork}
          disabled={!listener.can_isolate}
        />
        {t('aw.vocals_nodes')}
      </label>
      <p class="small muted sub">
        {#if listener.can_isolate}
          {t('aw.vocals_nodes_note')}
        {:else}
          {t('aw.vocals_needs')}
        {/if}
      </p>
    {/if}
    <div class="actions">
      <button onclick={() => act(api.installAiListener)} disabled={listener.installing}>
        {listener.installed ? t('aw.reinstall') : t('aw.install')}
      </button>
      {#if listener.installed}
        {#if listener.running}
          <button onclick={() => act(api.stopAiListener, t('aw.stopped'))}>{t('common.stop')}</button>
        {:else}
          <button class="primary" onclick={() => act(api.startAiListener, t('aw.started'))}>{t('aw.start')}</button>
        {/if}
      {/if}
      <span class="small"
        >{listener.installing
          ? t('aw.installing')
          : listener.running
            ? listener.stripping
              ? t('aw.running_both')
              : t('aw.running')
            : listener.installed
              ? t('aw.idle')
              : t('aw.not_installed')}</span
      >
    </div>
    {#if listener.error}<p class="small bad">{listener.error}</p>{/if}
    {#if listener.log.length}<pre class="log small">{listener.log.join('\n')}</pre>{/if}
  {/if}

  {#if work?.helpers.length}
    <h4>{t('aw.nodes_helping')}</h4>
    <ul class="helpers">
      {#each work.helpers as h (h.node_id)}
        {@const shown = helperState(h, work.on)}
        <li>
          <span class="light {shown.light}" role="img" aria-label={t(`aw.light_${shown.light}`)} title={t(`aw.light_${shown.light}`)}></span>
          <div class="helper">
            <strong>{h.name}</strong>
            <span class="small">{shown.text}</span>
            <span class="small muted">
              {t('aw.h_counts', { done: n(h.done), failed: n(h.failed) })}{#if h.asked_at}
                · {t('aw.h_asked', { when: new Date(h.asked_at).toLocaleTimeString(locale.code) })}{/if}{h.secs_per_minute
                ? t('aw.per_minute', { n: Math.round(h.secs_per_minute) })
                : ''}
            </span>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="small muted">
    {t('aw.nodes_note')} <a href="#/admin/nodes">{t('admin.menu.nodes')}</a>.
  </p>

  <h4>{t('aw.others')}</h4>
  <p class="small muted">
    {t('aw.others_1')}
    <code>ai-listener</code> {t('aw.others_2')}
    <code>python worker.py --server http://this-computer:8066 --key KEY</code>.
  </p>
  {#if work}
    <ul class="keys">
      {#each work.keys.filter((k) => k !== 'this computer') as name (name)}
        <li>
          <strong>{name}</strong>
          {#each work.workers.filter((w) => w.name === name) as w (w.name)}
            <span class="small muted"
              >{t('aw.seen', {
                when: new Date(w.last_seen).toLocaleTimeString(locale.code),
                done: n(w.done),
                failed: n(w.failed),
              })}</span
            >
          {/each}
          <button class="small-button" onclick={() => act(() => api.removeAiWorkerKey(name))}>{t('common.remove')}</button>
        </li>
      {/each}
    </ul>
    <div class="actions">
      <input placeholder={t('aw.computer_name')} bind:value={newKeyName} />
      <button onclick={makeKey} disabled={!newKeyName.trim()}>{t('aw.make_key')}</button>
    </div>
    {#if madeKey}
      <p class="small">
        {t('aw.key_once', { name: madeKey.name })} <code>{madeKey.key}</code>
      </p>
    {/if}
  {/if}
</section>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 6px 0;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 4px 2px 10px;
    border: 1px solid var(--edge);
    border-radius: 999px;
    font-size: 0.85rem;
  }

  .chip button {
    min-height: 0;
    padding: 0 6px;
  }

  .add-genre {
    display: flex;
    gap: 8px;
    margin: 6px 0 10px;
  }

  .add-genre input {
    flex: 1;
    min-width: 0;
  }

  .range {
    display: grid;
    gap: 4px;
    margin-top: 10px;
  }

  .range input {
    width: 100%;
  }

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
  .helpers {
    list-style: none;
    margin: 0.5rem 0;
    padding: 0;
    display: grid;
    gap: 0.6rem;
  }

  .helpers li {
    display: flex;
    gap: 0.7rem;
    align-items: flex-start;
  }

  .helper {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    min-width: 0;
  }

  .light {
    flex: none;
    width: 0.8rem;
    height: 0.8rem;
    margin-top: 0.3rem;
    border-radius: 50%;
    background: #8a8a96;
  }

  .light.red {
    background: #ff4d6a;
    box-shadow: 0 0 8px #ff4d6a;
  }

  .light.yellow {
    background: #ffd23f;
    box-shadow: 0 0 8px #ffd23f;
  }

  .light.green {
    background: #3ddc84;
    box-shadow: 0 0 10px #3ddc84;
    animation: working 1.6s ease-in-out infinite;
  }

  @keyframes working {
    50% {
      box-shadow: 0 0 2px #3ddc84;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .light.green {
      animation: none;
    }
  }
</style>
