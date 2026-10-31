<script lang="ts">
  // Your own account: username, password, two-step sign-in, and where you're signed
  // in. Used by the party app's Account page and by DJINN GO.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { deviceName } from './format'
  import { app } from './state.svelte'
  import TwoStepPanel from './TwoStepPanel.svelte'
  import type { Device } from './types'

  let username = $state(app.me?.username ?? '')
  let usernamePassword = $state('')
  let usernameMessage = $state('')
  let usernameError = $state('')
  let usernameBusy = $state(false)
  let current = $state('')
  let next = $state('')
  let confirm = $state('')
  let passwordMessage = $state('')
  let passwordError = $state('')
  let devices = $state<Device[]>([])
  let busy = $state(false)

  onMount(loadDevices)

  async function loadDevices() {
    try {
      devices = await api.myDevices()
    } catch {
      devices = []
    }
  }

  async function changeUsername(e: SubmitEvent) {
    e.preventDefault()
    usernameMessage = ''
    usernameError = ''
    usernameBusy = true
    try {
      const updated = await api.changeUsername(username, usernamePassword)
      app.me = updated
      username = updated.username
      usernamePassword = ''
      usernameMessage = 'Username changed. Use it for your next sign-in and in any phone apps.'
    } catch (err) {
      usernameError = err instanceof Error ? err.message : 'Could not change username'
    } finally {
      usernameBusy = false
    }
  }

  async function changePassword(e: SubmitEvent) {
    e.preventDefault()
    passwordMessage = ''
    passwordError = ''
    if (next !== confirm) {
      passwordError = 'The new passwords do not match'
      return
    }
    busy = true
    try {
      await api.changePassword(current, next)
      current = next = confirm = ''
      passwordMessage = 'Password changed. Your other devices are signed out.'
      await loadDevices()
    } catch (err) {
      passwordError = err instanceof Error ? err.message : 'Could not change it'
    } finally {
      busy = false
    }
  }

  async function signOut(device: Device) {
    try {
      await api.signOutMyDevice(device.id)
      devices = devices.filter((d) => d.id !== device.id)
    } catch {
      await loadDevices()
    }
  }

  async function signOutOthers() {
    try {
      await api.signOutMyOtherDevices()
    } finally {
      await loadDevices()
    }
  }

  async function twoStepChanged(on: boolean) {
    if (app.me) app.me = { ...app.me, two_step: on }
    // Turning it on signs out your other devices.
    await loadDevices()
  }

  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

{#if app.me?.role !== 'guest'}
  <section class="card">
    <h2>Username</h2>
    <p class="small muted">Your sign-in name also goes in phone apps connected to DJINN GO.</p>
    <form class="stack" onsubmit={changeUsername}>
      <label>
        Username
        <input bind:value={username} maxlength="32" autocomplete="username" autocapitalize="none" required />
      </label>
      <label>
        Current password
        <input type="password" bind:value={usernamePassword} autocomplete="current-password" required />
      </label>
      {#if usernameError}<p class="error">{usernameError}</p>{/if}
      {#if usernameMessage}<p class="muted">{usernameMessage}</p>{/if}
      <button class="primary" disabled={usernameBusy}>Change username</button>
    </form>
  </section>
{/if}

<section class="card">
  <h2>Password</h2>
  <form class="stack" onsubmit={changePassword}>
    <label>
      Current password
      <input type="password" bind:value={current} autocomplete="current-password" required />
    </label>
    <label>
      New password
      <input type="password" bind:value={next} autocomplete="new-password" minlength="8" required />
    </label>
    <label>
      New password again
      <input type="password" bind:value={confirm} autocomplete="new-password" required />
    </label>
    {#if passwordError}<p class="error">{passwordError}</p>{/if}
    {#if passwordMessage}<p class="muted">{passwordMessage}</p>{/if}
    <button class="primary" disabled={busy}>Change password</button>
  </form>
</section>

<TwoStepPanel onchange={twoStepChanged} />

<section class="card">
  <h2>Signed in on</h2>
  {#each devices as device (device.id)}
    <div class="device">
      <div class="grow">
        <div>{deviceName(device.user_agent)} {#if device.current}<span class="muted small">(this one)</span>{/if}</div>
        <div class="muted small">{device.ip ?? 'Unknown address'}, last used {when(device.last_seen_at)}</div>
      </div>
      {#if !device.current}
        <button class="small-button" onclick={() => signOut(device)}>Sign out</button>
      {/if}
    </div>
  {/each}
  {#if devices.length > 1}
    <button onclick={signOutOthers}>Sign out everywhere else</button>
  {/if}
</section>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .stack p {
    margin: 0;
  }

  .device {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }
</style>
