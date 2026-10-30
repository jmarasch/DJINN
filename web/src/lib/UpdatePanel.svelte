<script lang="ts">
  // Admin > System: checking for, and installing, new versions.
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { About } from './types'

  let about = $state<About | null>(null)
  let checking = $state(false)
  /** The update is installed and DJINN is starting again. */
  let restarting = $state(false)

  $effect(() => {
    api
      .about()
      .then((a) => (about = a))
      .catch((e) => toast(e.message, true))
  })

  // While an update downloads, follow along; once DJINN goes away to
  // restart, wait for it and reload into the new version.
  $effect(() => {
    if (!about?.installing && !restarting) return
    const started = about?.version
    const timer = setInterval(async () => {
      try {
        const now = await api.about()
        if (restarting && now.version !== started) location.reload()
        about = now
      } catch {
        restarting = true
      }
    }, 1500)
    return () => clearInterval(timer)
  })

  async function check() {
    checking = true
    try {
      about = await api.checkForUpdate()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not check for updates', true)
    } finally {
      checking = false
    }
  }

  async function install() {
    if (!about?.latest) return
    if (!confirm(`Install DJINN ${about.latest.version}? The music stops for a few seconds while DJINN restarts.`)) return
    try {
      await api.installUpdate()
      about = await api.about()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not install the update', true)
    }
  }

  async function setUpdates(change: Partial<Pick<About, 'check_updates' | 'auto_update'>>) {
    if (!about) return
    const next = { check_updates: about.check_updates, auto_update: about.auto_update, ...change }
    try {
      await api.setUpdateSettings(next)
      about = { ...about, ...next }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  const mb = (bytes: number) => `${(bytes / 1_000_000).toFixed(1)} MB`
  function ago(ms: number): string {
    const minutes = Math.round((Date.now() - ms) / 60_000)
    if (minutes < 1) return 'just now'
    if (minutes < 60) return `${minutes} min ago`
    const hours = Math.round(minutes / 60)
    if (hours < 24) return `${hours} h ago`
    return new Date(ms).toLocaleDateString()
  }
</script>

{#if about}
  <section class="card">
    <h2>Updates</h2>
    {#if restarting}
      <p class="small"><span class="spinner" aria-hidden="true"></span> Restarting with the new version...</p>
    {:else if about.installing}
      <p class="small"><span class="spinner" aria-hidden="true"></span> Installing DJINN {about.latest?.version}...</p>
      {#if about.progress}
        <p class="small muted">{mb(about.progress[0])} of {mb(about.progress[1])}</p>
        <progress max={about.progress[1]} value={about.progress[0]}></progress>
      {/if}
    {:else if about.update_available && about.latest}
      <p class="small"><strong>DJINN {about.latest.version} is available.</strong></p>
      {#if about.latest.notes}
        <details>
          <summary class="small">What's new</summary>
          <p class="small notes">{about.latest.notes}</p>
        </details>
      {/if}
      {#if about.cannot_install}
        <p class="small muted">{about.cannot_install}</p>
        <a class="small" href={about.latest.url} target="_blank" rel="noopener noreferrer">Download it from GitHub</a>
      {:else}
        <button class="primary" onclick={install}>Install and restart</button>
      {/if}
    {:else if about.checked_at}
      <p class="small muted">
        {about.latest ? "You're up to date." : 'No releases published yet.'} Checked {ago(about.checked_at)}.
      </p>
    {:else}
      <p class="small muted">Not checked yet.</p>
    {/if}
    {#if about.error}<p class="small error">{about.error}</p>{/if}
    {#if !about.installing && !restarting}
      <button class="small-button" disabled={checking} onclick={check}>
        {checking ? 'Checking...' : 'Check now'}
      </button>
    {/if}

    <div class="options">
      <label class="switch">
        <input
          type="checkbox"
          checked={about.check_updates}
          onchange={(e) => setUpdates({ check_updates: e.currentTarget.checked })}
        />
        Check for updates once a day
      </label>
      <label class="switch">
        <input
          type="checkbox"
          checked={about.auto_update}
          disabled={!about.check_updates}
          onchange={(e) => setUpdates({ auto_update: e.currentTarget.checked })}
        />
        Install updates by themselves, only while nothing is playing
      </label>
      <p class="small muted">
        Checking asks GitHub for the latest release and sends nothing but DJINN's version number. Updates are only
        installed if they're signed with DJINN's release key.
      </p>
    </div>
  </section>

{/if}

<style>
  progress {
    width: 100%;
    accent-color: var(--accent);
  }

  .error {
    color: var(--bad);
  }

  .notes {
    max-height: 240px;
    overflow-y: auto;
    white-space: pre-wrap;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--line);
  }

  .small-button {
    align-self: flex-start;
  }
</style>
