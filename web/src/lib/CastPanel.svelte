<script lang="ts">
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { CastDevice } from './types'

  let searching = $state(false)

  $effect(() => {
    api
      .casts()
      .then((list) => {
        app.casts = list.devices
        // First visit: look around right away.
        if (!list.devices.length) find()
      })
      .catch((e) => toast(e.message, true))
  })

  async function find() {
    searching = true
    try {
      app.casts = (await api.discoverCasts()).devices
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not look for devices', true)
    } finally {
      searching = false
    }
  }

  async function toggle(device: CastDevice) {
    try {
      const list = device.active ? await api.stopCast(device.id) : await api.startCast(device.id)
      app.casts = list.devices
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  async function stopAll() {
    try {
      await api.stopAllCasts()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  function setVolume(device: CastDevice, level: number) {
    device.volume = level
    clearTimeout(timers.get(device.id))
    timers.set(
      device.id,
      setTimeout(() => api.setCastVolume(device.id, level).catch((e) => toast(e.message, true)), 250),
    )
  }

  const kindLabel = (d: CastDevice) =>
    d.group ? 'Speaker group' : d.kind === 'cast' ? 'Google Cast' : d.kind === 'sonos' ? 'Sonos' : 'DLNA'

  function status(d: CastDevice): string {
    if (!d.active) return d.model && !d.group ? `${kindLabel(d)} · ${d.model}` : kindLabel(d)
    if (d.error) return `${d.error}. Trying again...`
    if (d.playing) return 'Playing the party'
    return 'Connecting...'
  }

  const anyActive = $derived(app.casts?.some((d) => d.active) ?? false)
</script>

<section class="card">
  <h2>Cast to TVs and speakers</h2>
  <p class="small muted">
    Play the party on Chromecasts, Google TVs, Nest speakers, smart TVs (DLNA), and Sonos, several at once. TVs show
    the join code while they play. Network devices run a few seconds behind this computer's speakers and can't be
    synced to them, so use either casts or local speakers in a room, not both. Speaker groups from the Google Home app
    stay in sync with each other.
  </p>

  {#if app.casts?.length}
    {#each app.casts as device (device.id)}
      <div class="device" class:active={device.active}>
        <span
          class="dot"
          class:playing={device.active && device.playing}
          class:failed={device.active && !!device.error}
          aria-hidden="true"
        ></span>
        <div class="grow">
          <div class="name">{device.name}</div>
          <div class="small muted" class:error={device.active && !!device.error}>{status(device)}</div>
          {#if device.active && device.volume !== null && !device.volume_fixed}
            <label class="volume">
              <span class="small muted">Volume {Math.round(device.volume * 100)}</span>
              <input
                type="range"
                min="0"
                max="1"
                step="0.01"
                value={device.volume}
                oninput={(e) => setVolume(device, Number(e.currentTarget.value))}
              />
            </label>
          {:else if device.active && device.volume_fixed}
            <div class="small muted">Volume is set on the TV.</div>
          {/if}
        </div>
        <button class:primary={!device.active} onclick={() => toggle(device)}>
          {device.active ? 'Stop' : 'Cast'}
        </button>
      </div>
    {/each}
  {:else if !searching}
    <p class="muted">
      No devices found. They need to be on the same Wi-Fi as this computer, and switched on.
    </p>
  {/if}

  <div class="row-buttons">
    <button disabled={searching} onclick={find}>
      {#if searching}<span class="spinner" aria-hidden="true"></span> Looking...{:else}Look for devices{/if}
    </button>
    {#if anyActive}<button onclick={stopAll}>Stop all casts</button>{/if}
  </div>
</section>

<style>
  .device {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
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

  .volume {
    gap: 0;
    margin-top: 4px;
  }

  .device button {
    flex: none;
    min-width: 76px;
  }

  .row-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    margin-right: 6px;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    vertical-align: -1px;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
