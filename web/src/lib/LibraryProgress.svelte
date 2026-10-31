<script lang="ts">
  // What DJINN is doing with the library, in steps: checking the folders
  // (find, read, sort), then analyzing the sound, then AI listening if it's
  // on. Artist lookups run alongside and get their own line. Hidden when
  // nothing is happening.
  import { heldText } from './background'
  import { app } from './state.svelte'

  type Step = { key: string; label: string }

  const scanning = $derived(!!app.scan && !app.scan.done)
  const analyzing = $derived(!!app.loudness?.running)
  const listening = $derived(!!app.listening?.running)
  const lookingUp = $derived(!!app.lookup?.running)

  const steps = $derived<Step[]>([
    { key: 'finding', label: 'Find music' },
    { key: 'reading', label: 'Read songs' },
    { key: 'sorting', label: 'Sort albums and artists' },
    ...(app.loudness?.on ? [{ key: 'analysis', label: 'Analyze sound' }] : []),
    ...(app.listening?.on ? [{ key: 'listening', label: 'AI listening' }] : []),
  ])

  /** The step running now. */
  const current = $derived(
    scanning ? (app.scan?.phase ?? 'finding') : analyzing ? 'analysis' : listening ? 'listening' : null,
  )
  const index = $derived(current ? steps.findIndex((s) => s.key === current) : -1)
  /** What the step running now waits for, if it waits (music playing, or outside its hours). */
  const held = $derived(
    current === 'analysis' ? app.loudness?.held : current === 'listening' ? app.listening?.held : null,
  )

  const n = (x: number) => x.toLocaleString()

  /** [done, total] for a progress bar, or null when it can't be counted. */
  const amount = $derived.by((): [number, number] | null => {
    if (current === 'reading' && app.scan) return [app.scan.read, app.scan.to_read]
    if (current === 'analysis' && app.loudness)
      return [app.loudness.analyzed, app.loudness.analyzed + app.loudness.waiting]
    if (current === 'listening' && app.listening)
      return [app.listening.heard, app.listening.heard + app.listening.waiting]
    return null
  })

  const detail = $derived.by(() => {
    switch (current) {
      case 'finding':
        return 'Looking through your music folders'
      case 'reading':
        return amount ? `${n(amount[0])} of ${n(amount[1])} new or changed files` : ''
      case 'sorting':
        return `${n(app.scan?.found ?? 0)} songs found; picking album and artist pictures`
      case 'analysis':
        return amount
          ? `${n(amount[0])} of ${n(amount[1])} songs, for "more like this", radio and even volume`
          : ''
      case 'listening':
        return amount ? `${n(amount[0])} of ${n(amount[1])} songs, for genres and searching by sound` : ''
      default:
        return ''
    }
  })
</script>

{#if current || lookingUp}
  <section class="progress-card" aria-live="polite">
    {#if current && index >= 0}
      <div class="head">
        {#if !held}<span class="spinner" aria-hidden="true"></span>{/if}
        <b>Step {index + 1} of {steps.length}: {steps[index].label}</b>
      </div>
      {#if held}<p class="small">{heldText(held)}.</p>{/if}
      {#if detail}<p class="small muted">{detail}</p>{/if}
      {#if amount && amount[1] > 0}
        <progress max={amount[1]} value={Math.min(amount[0], amount[1])}></progress>
      {:else}
        <progress></progress>
      {/if}
      <ol class="steps">
        {#each steps as step, i (step.key)}
          <li class:done={i < index} class:now={i === index}>
            <span class="dot" aria-hidden="true">{i < index ? '✓' : i + 1}</span>
            {step.label}
          </li>
        {/each}
      </ol>
      {#if index >= 3}
        <p class="small muted">Your music is ready to play; this carries on in the background.</p>
      {/if}
    {/if}
    {#if lookingUp}
      <p class="small muted lookup">
        {#if !current}<span class="spinner" aria-hidden="true"></span>{/if}
        Also looking up artists online
      </p>
    {/if}
  </section>
{/if}

<style>
  .progress-card {
    display: grid;
    gap: 6px;
    margin: 0 0 12px;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--surface);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  p {
    margin: 0;
  }

  progress {
    width: 100%;
  }

  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    margin: 2px 0 0;
    padding: 0;
    list-style: none;
    font-size: 0.8125rem;
    color: var(--muted);
  }

  .steps li {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .steps li.now {
    color: inherit;
    font-weight: 600;
  }

  .dot {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 999px;
    border: 1px solid var(--line);
    font-size: 0.6875rem;
  }

  .done .dot {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }

  .now .dot {
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
