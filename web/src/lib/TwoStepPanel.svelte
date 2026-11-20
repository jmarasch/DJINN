<script lang="ts">
  // Two-step sign-in for your own account: scan a QR code into an
  // authenticator app, confirm a code, keep the recovery codes.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { t } from './i18n.svelte'
  import type { TwoStepSetup, TwoStepStatus } from './types'

  let { onchange }: { onchange?: (on: boolean) => void } = $props()

  let status = $state<TwoStepStatus | null>(null)
  let setup = $state<TwoStepSetup | null>(null)
  let code = $state('')
  let password = $state('')
  let recovery = $state<string[] | null>(null)
  let error = $state('')
  let busy = $state(false)
  let turningOff = $state(false)
  /** Asking for the password before showing a new code to scan. */
  let starting = $state(false)

  onMount(load)

  async function load() {
    try {
      status = await api.twoStep()
    } catch (e) {
      error = e instanceof Error ? e.message : t('uf.load_failed')
    }
  }

  async function run(action: () => Promise<void>) {
    busy = true
    error = ''
    try {
      await action()
    } catch (e) {
      error = e instanceof Error ? e.message : t('common.failed')
    } finally {
      busy = false
    }
  }

  const begin = (e: SubmitEvent) => {
    e.preventDefault()
    return run(async () => {
      setup = await api.startTwoStep(password)
      password = ''
      starting = false
    })
  }

  const confirm = (e: SubmitEvent) => {
    e.preventDefault()
    return run(async () => {
      recovery = (await api.confirmTwoStep(code)).recovery_codes
      setup = null
      code = ''
      await load()
      onchange?.(true)
    })
  }

  const turnOff = (e: SubmitEvent) => {
    e.preventDefault()
    return run(async () => {
      await api.turnOffTwoStep(password)
      password = ''
      turningOff = false
      await load()
      onchange?.(false)
    })
  }

  async function copyCodes() {
    if (!recovery) return
    try {
      await navigator.clipboard.writeText(recovery.join('\n'))
    } catch {
      // Selecting them by hand still works.
    }
  }
</script>

<section class="card">
  <h2>{t('ts.title')}</h2>
  {#if !status}
    <p class="muted">{t('common.loading')}</p>
  {:else if recovery}
    <p>{t('ts.on_codes')}</p>
    <ul class="codes">
      {#each recovery as c (c)}<li>{c}</li>{/each}
    </ul>
    <div class="actions">
      <button onclick={copyCodes}>{t('ts.copy')}</button>
      <button class="primary" onclick={() => (recovery = null)}>{t('ts.saved')}</button>
    </div>
  {:else if setup}
    <p>{t('ts.scan')}</p>
    <!-- The QR code is an SVG DJINN made itself. -->
    <div class="qr">{@html setup.qr_svg}</div>
    <p class="small muted">{t('ts.cant_scan')} <span class="mono">{setup.secret}</span></p>
    <form class="actions" onsubmit={confirm}>
      <input bind:value={code} inputmode="numeric" autocomplete="one-time-code" maxlength="7" placeholder="123456" required aria-label={t('ts.code_label')} />
      <button class="primary" disabled={busy}>{t('ts.turn_on')}</button>
    </form>
    <button class="link" onclick={() => (setup = null)}>{t('common.cancel')}</button>
  {:else if status.on}
    <p>{t('ts.on')} {t('ts.left', { n: status.recovery_codes_left })}</p>
    {#if starting}
      <form class="actions" onsubmit={begin}>
        <input type="password" bind:value={password} autocomplete="current-password" placeholder={t('ts.your_password')} required aria-label={t('ts.your_password')} />
        <button class="primary" disabled={busy}>{t('ts.continue')}</button>
      </form>
    {:else if turningOff}
      <form class="actions" onsubmit={turnOff}>
        <input type="password" bind:value={password} autocomplete="current-password" placeholder={t('ts.your_password')} required aria-label={t('ts.your_password')} />
        <button class="danger" disabled={busy}>{t('ts.turn_off')}</button>
      </form>
      {#if status.required}<p class="small muted">{t('ts.go_needs_off')}</p>{/if}
    {:else}
      <div class="actions">
        <button onclick={() => (starting = true)} disabled={busy}>{t('ts.new_phone')}</button>
        <button class="danger" onclick={() => (turningOff = true)}>{t('ts.turn_off')}</button>
      </div>
    {/if}
  {:else}
    <p>
      {status.required ? t('ts.go_needs') : t('ts.off')}
      {t('ts.why')}
    </p>
    {#if starting}
      <form class="actions" onsubmit={begin}>
        <input type="password" bind:value={password} autocomplete="current-password" placeholder={t('ts.your_password')} required aria-label={t('ts.your_password')} />
        <button class="primary" disabled={busy}>{t('ts.continue')}</button>
      </form>
    {:else}
      <button class="primary" onclick={() => (starting = true)} disabled={busy}>{t('ts.set_up')}</button>
    {/if}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</section>

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .actions input {
    flex: 1;
    min-width: 0;
  }

  .qr {
    align-self: center;
    width: 220px;
    padding: 10px;
    border-radius: 12px;
    background: #fff;
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }

  .codes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px 16px;
    margin: 0;
    padding: 12px;
    border-radius: 10px;
    background: var(--surface-2);
    list-style: none;
    font-family: ui-monospace, monospace;
  }

  .mono {
    font-family: ui-monospace, monospace;
    overflow-wrap: anywhere;
  }

  p {
    margin: 0;
  }

  .link {
    align-self: flex-start;
    border: none;
    background: none;
    color: var(--muted);
    text-decoration: underline;
  }
</style>
