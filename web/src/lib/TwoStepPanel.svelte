<script lang="ts">
  // Two-step sign-in for your own account: scan a QR code into an
  // authenticator app, confirm a code, keep the recovery codes.
  import { onMount } from 'svelte'
  import { api } from './api'
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

  onMount(load)

  async function load() {
    try {
      status = await api.twoStep()
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not load'
    }
  }

  async function run(action: () => Promise<void>) {
    busy = true
    error = ''
    try {
      await action()
    } catch (e) {
      error = e instanceof Error ? e.message : 'That did not work'
    } finally {
      busy = false
    }
  }

  const begin = () =>
    run(async () => {
      setup = await api.startTwoStep()
    })

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
  <h2>Two-step sign-in</h2>
  {#if !status}
    <p class="muted">Loading...</p>
  {:else if recovery}
    <p>Two-step sign-in is on. Keep these recovery codes somewhere safe, like a password manager. Each one works once if you lose your phone. They aren't shown again.</p>
    <ul class="codes">
      {#each recovery as c (c)}<li>{c}</li>{/each}
    </ul>
    <div class="row">
      <button onclick={copyCodes}>Copy codes</button>
      <button class="primary" onclick={() => (recovery = null)}>I've saved them</button>
    </div>
  {:else if setup}
    <p>Scan this with an authenticator app (like Google Authenticator, Microsoft Authenticator, 1Password or Bitwarden), then type the 6-digit code it shows.</p>
    <!-- The QR code is an SVG DJINN made itself. -->
    <div class="qr">{@html setup.qr_svg}</div>
    <p class="small muted">Can't scan? Type this key into the app: <span class="mono">{setup.secret}</span></p>
    <form class="row" onsubmit={confirm}>
      <input bind:value={code} inputmode="numeric" autocomplete="one-time-code" maxlength="7" placeholder="123456" required aria-label="Code from the app" />
      <button class="primary" disabled={busy}>Turn on</button>
    </form>
    <button class="link" onclick={() => (setup = null)}>Cancel</button>
  {:else if status.on}
    <p>On. Signing in asks for a code from your authenticator app. {status.recovery_codes_left} recovery {status.recovery_codes_left === 1 ? 'code' : 'codes'} left.</p>
    {#if turningOff}
      <form class="row" onsubmit={turnOff}>
        <input type="password" bind:value={password} autocomplete="current-password" placeholder="Your password" required aria-label="Your password" />
        <button disabled={busy}>Turn off</button>
      </form>
      {#if status.required}<p class="small muted">DJINN GO needs it for your account, so GO stops working until you turn it on again.</p>{/if}
    {:else}
      <div class="row">
        <button onclick={begin} disabled={busy}>New phone or new codes</button>
        <button onclick={() => (turningOff = true)}>Turn off</button>
      </div>
    {/if}
  {:else}
    <p>
      {status.required ? 'DJINN GO needs this for your account.' : 'Off.'}
      After your password, signing in also asks for a code from an app on your phone, so a stolen password isn't enough.
    </p>
    <button class="primary" onclick={begin} disabled={busy}>Set up two-step sign-in</button>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</section>

<style>
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .row input {
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
