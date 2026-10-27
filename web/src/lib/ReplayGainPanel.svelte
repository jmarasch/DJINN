<script lang="ts">
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { ReplayGainMode } from './types'

  let mode = $state<ReplayGainMode>('off')
  let preamp = $state(0)
  let maxPreamp = $state(12)
  let loaded = $state(false)

  $effect(() => {
    api
      .replayGain()
      .then((r) => {
        mode = r.mode
        preamp = r.preamp_db
        maxPreamp = r.max_preamp_db
        app.loudness = r.loudness
        loaded = true
      })
      .catch((e) => toast(e.message, true))
  })

  const modes: { id: ReplayGainMode; label: string; detail: string }[] = [
    { id: 'off', label: 'Off', detail: 'Songs play as loud as they were made.' },
    { id: 'track', label: 'Even out every song', detail: 'Best for parties and shuffled playlists.' },
    {
      id: 'album',
      label: 'Even out albums',
      detail: 'Quiet and loud songs on an album keep their difference. Good for playing whole albums.',
    },
  ]

  let timer: ReturnType<typeof setTimeout>
  function save(wait = 0) {
    clearTimeout(timer)
    timer = setTimeout(async () => {
      try {
        const r = await api.saveReplayGain(mode, preamp)
        app.loudness = r.loudness
      } catch (e) {
        toast(e instanceof Error ? e.message : 'Could not save', true)
      }
    }, wait)
  }

  const progress = $derived(app.loudness)
  const total = $derived(progress ? progress.analyzed + progress.waiting : 0)
</script>

<section class="card">
  <h2>Volume leveling (ReplayGain)</h2>
  <p class="small muted">
    Keeps songs at an even loudness, so a quiet old track isn't followed by a blast. Uses ReplayGain tags when a file
    has them. For files without, DJINN measures each song once and keeps the result in its own database; your files
    aren't changed. Applies from the next song.
  </p>
  {#if loaded}
    <div class="modes" role="radiogroup" aria-label="Volume leveling">
      {#each modes as m (m.id)}
        <label class="mode" class:active={mode === m.id}>
          <input type="radio" name="replaygain" value={m.id} bind:group={mode} onchange={() => save()} />
          <span>
            <span class="label">{m.label}</span>
            <span class="small muted">{m.detail}</span>
          </span>
        </label>
      {/each}
    </div>

    {#if mode !== 'off'}
      <label class="slider">
        <span class="row-label">
          Boost <span class="muted">{preamp > 0 ? '+' : ''}{preamp.toFixed(1)} dB</span>
        </span>
        <input type="range" min={-maxPreamp} max={maxPreamp} step="0.5" bind:value={preamp} oninput={() => save(400)} />
        <span class="small muted">
          Leveled songs play quieter than most modern masters. Raise this if everything sounds too soft; songs never
          get pushed into clipping.
        </span>
      </label>

      {#if progress && total > 0}
        {#if progress.waiting > 0}
          <p class="small">
            {#if progress.running}<span class="spinner" aria-hidden="true"></span>{/if}
            Measured {progress.analyzed} of {total} songs{progress.running ? '' : ' (continues after the next library check)'}.
            Songs not measured yet play unchanged.
          </p>
          <progress max={total} value={progress.analyzed}></progress>
        {:else}
          <p class="small muted">All {total} songs are measured.</p>
        {/if}
      {/if}
    {/if}
  {/if}
</section>

<style>
  .modes {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .mode {
    flex-direction: row;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    cursor: pointer;
  }

  .mode.active {
    border-color: var(--accent);
  }

  .mode input {
    flex: none;
    width: 20px;
    min-height: 20px;
    margin: 1px 0 0;
    accent-color: var(--accent);
  }

  .mode > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .label {
    font-weight: 600;
  }

  .slider {
    gap: 4px;
  }

  .row-label {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    font-size: 0.875rem;
  }

  progress {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
