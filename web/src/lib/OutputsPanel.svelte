<script lang="ts">
  import { api } from './api'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import { app, toast } from './state.svelte'
  import type { AudioDevice, LiveOutput, OutputConfig } from './types'

  let outputs = $state<OutputConfig[]>([])
  let maxDelay = $state(1000)
  let devices = $state<AudioDevice[] | null>(null)
  let adding = $state(false)

  $effect(() => {
    api
      .outputs()
      .then((list) => {
        outputs = list.outputs
        maxDelay = list.max_delay_ms
        app.output = list.status
      })
      .catch((e) => toast(e.message, true))
  })

  const live = (id: number): LiveOutput | undefined => app.output.outputs.find((o) => o.id === id)
  const playingCount = $derived(app.output.outputs.filter((o) => o.state === 'playing').length)

  const timers = new Map<number, ReturnType<typeof setTimeout>>()

  /** Saves after sliders settle, so dragging doesn't flood the server. */
  function save(output: OutputConfig, wait = 250) {
    clearTimeout(timers.get(output.id))
    timers.set(
      output.id,
      setTimeout(async () => {
        try {
          await api.updateOutput(output)
        } catch (e) {
          toast(e instanceof Error ? e.message : 'Could not save', true)
        }
      }, wait),
    )
  }

  function nudge(output: OutputConfig, ms: number) {
    output.delay_ms = Math.min(maxDelay, Math.max(0, output.delay_ms + ms))
    save(output, 0)
  }

  async function openPicker() {
    adding = true
    devices = null
    try {
      devices = await api.audioDevices()
    } catch (e) {
      adding = false
      toast(e instanceof Error ? e.message : 'Could not list devices', true)
    }
  }

  async function add(device: AudioDevice) {
    try {
      const output = await api.addOutput(device.id, device.name)
      if (!outputs.some((o) => o.id === output.id)) outputs.push(output)
      adding = false
      toast(`Added ${output.label}`)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add it', true)
    }
  }

  async function remove(output: OutputConfig) {
    if (!confirm(`Stop playing through ${output.label} and remove it?`)) return
    try {
      await api.removeOutput(output.id)
      outputs = outputs.filter((o) => o.id !== output.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove it', true)
    }
  }

  function statusText(output: OutputConfig, l: LiveOutput | undefined): string {
    if (!output.enabled) return 'Off'
    if (!l || l.state === 'waiting') return l?.error ? `Not connected: ${l.error}` : 'Connecting...'
    const device = output.device_id ? '' : `${l.device} · `
    return `${device}${(l.sample_rate ?? 0) / 1000} kHz`
  }

  function syncText(l: LiveOutput): string {
    if (l.clock) return playingCount > 1 ? 'Others keep time with this one' : ''
    // A few milliseconds apart is far below what anyone can hear.
    const offset = Math.abs(l.offset_ms) < 5 ? 'In sync' : `${l.offset_ms > 0 ? 'Behind' : 'Ahead'} by ${Math.abs(l.offset_ms).toFixed(0)} ms, catching up`
    const speed = l.drift_ppm === 0 ? '' : ` · speed ${l.drift_ppm > 0 ? '+' : ''}${l.drift_ppm.toFixed(0)} ppm`
    return offset + speed
  }
</script>

<section class="card">
  <h2>Outputs</h2>
  <p class="small muted">
    Play through more than one device at once, like the stereo and a Bluetooth speaker in the next room. DJINN keeps
    them in time. Bluetooth lags a few hundred milliseconds behind wired outputs, so raise the delay on the wired ones
    until everything lines up.
  </p>

  {#each outputs as output (output.id)}
    {@const l = live(output.id)}
    <div class="output" class:off={!output.enabled}>
      <div class="head">
        <span
          class="dot"
          class:playing={l?.state === 'playing'}
          class:failed={output.enabled && l?.state === 'waiting' && !!l?.error}
          aria-hidden="true"
        ></span>
        <div class="grow">
          <div class="label">
            {output.label}
            {#if l?.clock && playingCount > 1}<span class="badge">Clock</span>{/if}
          </div>
          <div class="small muted" class:error={output.enabled && !!l?.error && l.state !== 'playing'}>
            {statusText(output, l)}
          </div>
          {#if l?.state === 'playing' && syncText(l)}
            <div class="small muted">{syncText(l)}</div>
          {/if}
        </div>
        <label class="switch">
          <input type="checkbox" bind:checked={output.enabled} onchange={() => save(output, 0)} />
          On
        </label>
        {#if output.device_id}
          <button class="icon" aria-label="Remove {output.label}" onclick={() => remove(output)}><Icon name="close" /></button>
        {/if}
      </div>

      <label class="slider">
        <span class="row-label">Volume <span class="muted">{Math.round(output.volume_trim * 100)}%</span></span>
        <input
          type="range"
          min="0"
          max="1"
          step="0.01"
          bind:value={output.volume_trim}
          oninput={() => save(output)}
          disabled={!output.enabled}
        />
      </label>

      <div class="slider">
        <label for="delay-{output.id}" class="row-label">Delay <span class="muted">{output.delay_ms} ms</span></label>
        <div class="delay">
          <button class="step" aria-label="10 ms less delay" disabled={!output.enabled} onclick={() => nudge(output, -10)}
            >-10</button
          >
          <input
            id="delay-{output.id}"
            type="range"
            min="0"
            max={maxDelay}
            step="5"
            bind:value={output.delay_ms}
            oninput={() => save(output)}
            disabled={!output.enabled}
          />
          <button class="step" aria-label="10 ms more delay" disabled={!output.enabled} onclick={() => nudge(output, 10)}
            >+10</button
          >
        </div>
      </div>
    </div>
  {/each}

  <button class="primary" onclick={openPicker}>Add an output</button>
</section>

{#if adding}
  <Sheet title="Add an output" onclose={() => (adding = false)}>
    {#if devices === null}
      <p class="muted">Looking for devices...</p>
    {:else}
      {#each devices as device (device.id)}
        <button class="device" disabled={device.added} onclick={() => add(device)}>
          <span class="grow">
            <span class="label">{device.name}</span>
            {#if device.added}
              <span class="small muted">Already added</span>
            {:else if device.is_default}
              <span class="small muted">The system default right now, which already plays through "System default"</span>
            {/if}
          </span>
          {#if !device.added}<Icon name="plus" />{/if}
        </button>
      {:else}
        <p class="muted">No other devices found. Connect a Bluetooth speaker or plug something in, then try again.</p>
      {/each}
      <p class="small muted">Bluetooth speakers show up here once they're paired and connected to this computer.</p>
    {/if}
  </Sheet>
{/if}

<style>
  .output {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  .output.off .slider {
    opacity: 0.5;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .label {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .badge {
    margin-left: 6px;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--surface-3);
    color: var(--muted);
    font-size: 0.75rem;
    font-weight: 500;
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--line);
  }

  .dot.playing {
    background: var(--ok);
  }

  .dot.failed {
    background: var(--bad);
  }

  .error {
    color: var(--bad);
  }

  .switch {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    font-size: 0.8125rem;
    color: var(--muted);
  }

  .switch input {
    width: 20px;
    min-height: 20px;
    accent-color: var(--accent);
  }

  .icon {
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .slider {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .row-label {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    font-size: 0.875rem;
  }

  .delay {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .delay input {
    flex: 1;
    min-width: 0;
  }

  .step {
    flex: none;
    min-height: 36px;
    padding: 0 10px;
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }

  .device {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    margin-bottom: 8px;
    padding: 12px;
    text-align: left;
  }

  .device .grow {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
</style>
