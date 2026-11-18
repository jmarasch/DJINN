<script lang="ts">
  import { t } from './i18n.svelte'
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
      toast(e instanceof Error ? e.message : t('cast.look_failed'), true)
    } finally {
      searching = false
    }
  }

  async function cast(device: CastDevice, mode: 'audio' | 'page') {
    try {
      app.casts = (await api.startCast(device.id, mode)).devices
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
  }

  async function stop(device: CastDevice) {
    try {
      app.casts = (await api.stopCast(device.id)).devices
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
  }

  async function stopAll() {
    try {
      await api.stopAllCasts()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
  }

  async function newLink() {
    if (!confirm(t('cast.new_link_q'))) return
    try {
      app.casts = (await api.newStreamLink()).devices
      toast(t('cast.new_link_done'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
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
    d.group ? t('cast.group') : d.kind === 'cast' ? 'Google Cast' : d.kind === 'sonos' ? 'Sonos' : 'DLNA'

  function status(d: CastDevice): string {
    if (!d.active) return d.model && !d.group ? `${kindLabel(d)} · ${d.model}` : kindLabel(d)
    if (d.error) return t('cast.retrying', { error: d.error })
    if (d.playing) return d.page ? t('cast.showing') : t('cast.playing')
    return t('out.connecting')
  }

  const anyActive = $derived(app.casts?.some((d) => d.active) ?? false)
</script>

<section class="card">
  <h2>{t('cast.title')}</h2>
  <p class="small muted">
    {t('cast.intro')}
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
              <span class="small muted">{t('cast.volume', { n: Math.round(device.volume * 100) })}</span>
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
            <div class="small muted">{t('cast.volume_tv')}</div>
          {/if}
        </div>
        <div class="actions">
          {#if device.active}
            {#if device.screen}
              <button class="small-button" onclick={() => cast(device, device.page ? 'audio' : 'page')}>
                {device.page ? t('cast.audio_only') : t('cast.show_page')}
              </button>
            {/if}
            <button onclick={() => stop(device)}>{t('common.stop')}</button>
          {:else if device.screen}
            <button class="primary" onclick={() => cast(device, 'page')}>{t('cast.show_on_tv')}</button>
            <button class="small-button" onclick={() => cast(device, 'audio')}>{t('cast.audio_only')}</button>
          {:else}
            <button class="primary" onclick={() => cast(device, 'audio')}>{t('cast.cast')}</button>
          {/if}
        </div>
      </div>
    {/each}
  {:else if !searching}
    <p class="muted">
      {t('cast.none')}
    </p>
  {/if}

  <div class="row-buttons">
    <button disabled={searching} onclick={find}>
      {#if searching}<span class="spinner" aria-hidden="true"></span> {t('cast.looking')}{:else}{t('cast.look')}{/if}
    </button>
    {#if anyActive}<button onclick={stopAll}>{t('cast.stop_all')}</button>{/if}
    <button onclick={newLink}>{t('cast.new_link')}</button>
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

  .actions {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
  }

  .actions button {
    min-width: 96px;
  }

  .small-button {
    min-height: 32px;
    padding: 0 10px;
  }

  .row-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
