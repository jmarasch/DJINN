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
  // The minimum score, kept apart from `work` so the 3-second refresh doesn't
  // move the slider under a finger.
  let minMatch = $state<number | null>(null)
  // Genres left out of AI lyrics, and every genre in the library to pick from.
  let skipGenres = $state<string[] | null>(null)
  let known = $state<string[]>([])
  let genreText = $state('')

  const n = (x: number) => x.toLocaleString()
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
      toast(e instanceof Error ? e.message : 'Could not save', true)
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
    <label class="range">
      <span>Keep lyrics that score at least <strong>{minMatch ?? work.min_match}%</strong></span>
      <input type="range" min="0" max="100" step="1" bind:value={minMatch} onchange={saveWork} />
    </label>
    <p class="small muted sub">
      For timing, the score is how well the words it heard match the lyrics. For lyrics it writes, it is how sure the
      model was of the words it heard (half the words at least this sure). Songs that score lower get no AI lyrics and count as failed; lower the minimum,
      then select Try again, to give them another go.
    </p>
    <h4>Genres to leave out</h4>
    <p class="small muted sub">
      Songs in these genres get no AI lyrics (a genre takes in its narrower ones, so "metal" covers "black metal"),
      unless you ask for a song in the library editor. Handy for instrumental or classical music, or vocals the AI
      can't follow.
    </p>
    {#if skipGenres?.length}
      <div class="chips">
        {#each skipGenres as genre (genre)}
          <span class="chip">
            {genre}
            <button class="icon-only" aria-label="Stop leaving out {genre}" onclick={() => removeGenre(genre)}>×</button>
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
      <input list="ai-skip-genres" bind:value={genreText} placeholder="A genre, like classical" aria-label="Genre to leave out" />
      <datalist id="ai-skip-genres">
        {#each known as g (g)}<option value={g}></option>{/each}
      </datalist>
      <button class="small-button" type="submit" disabled={!genreText.trim()}>Leave out</button>
    </form>
    <p class="small">
      Waiting: {n(work.counts.waiting_listen)} without lyrics, {n(work.counts.waiting_sync)} plain.
      Working: {n(work.counts.working)}.{#if work.counts.prepared}
        Vocals ready for nodes: {n(work.counts.prepared)}.{/if} Done: {n(work.counts.done)}. Failed: {n(work.counts.failed)}
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
    {#if work}
      <label class="switch">
        <input
          type="checkbox"
          bind:checked={work.vocals_for_nodes}
          onchange={saveWork}
          disabled={!listener.can_isolate}
        />
        Isolate the vocals here for nodes that help
      </label>
      <p class="small muted sub">
        {#if listener.can_isolate}
          This computer does the heavy part (separating the voice from the music) and sends a node only the voice, a
          few megabytes, to listen to. The node deletes it when done.
        {:else}
          Needs the listener installed here with "Isolate the vocals first". Without it, nodes listen to whole songs.
        {/if}
      </p>
    {/if}
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

  {#if work?.workers.some((w) => w.node)}
    <h4>Nodes helping</h4>
    <ul class="keys">
      {#each work.workers.filter((w) => w.node) as w (w.name)}
        <li>
          <strong>{w.label}</strong>
          <span class="small muted">
            asked {new Date(w.last_seen).toLocaleTimeString()} · {n(w.done)} done · {n(w.failed)} failed{w.secs_per_minute
              ? ` · ${Math.round(w.secs_per_minute)} s per minute of song`
              : ''}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="small muted">Nodes are switched on to help, and given their model, in <a href="#/admin/nodes">Nodes</a>.</p>

  <h4>Other computers</h4>
  <p class="small muted">
    A computer with a decent processor or graphics card can listen too. Make a key for it, copy the folder
    <code>ai-listener</code> from DJINN's data folder to it, and run
    <code>python worker.py --server http://this-computer:8066 --key KEY</code>.
  </p>
  {#if work}
    <ul class="keys">
      {#each work.keys.filter((k) => k !== 'this computer') as name (name)}
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
</style>
