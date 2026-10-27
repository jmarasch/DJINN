<script lang="ts">
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
    <h2>DJINN {about.version}</h2>
    <p class="small muted">
      A party jukebox by New Terra Studios.{#if about.commit}
        Build {about.commit}{about.commit_date ? `, ${about.commit_date}` : ''}.{/if}
    </p>
    <div class="links">
      <a href={about.repository} target="_blank" rel="noopener noreferrer">Source code on GitHub</a>
      <a href="{about.repository}/issues" target="_blank" rel="noopener noreferrer">Report a problem</a>
      <a href="{about.repository}/releases" target="_blank" rel="noopener noreferrer">All releases</a>
      <a href="mailto:{about.contact}">{about.contact}</a>
    </div>
  </section>

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

  <section class="card">
    <h2>Credits</h2>
    <ul class="small credits">
      <li>Music details from MusicBrainz and the Cover Art Archive; song identification by AcoustID.</li>
      <li>Artist photos from Deezer and Wikimedia Commons; bios from Wikipedia.</li>
      <li>AI listening: LAION's CLAP (Apache-2.0). AI reader: Qwen2.5 by Alibaba Cloud (Apache-2.0).</li>
      <li>Built with Rust, Svelte, and many open source libraries, including Hugging Face's candle (MIT or Apache-2.0).</li>
    </ul>
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

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
  }

  .notes {
    max-height: 240px;
    overflow-y: auto;
    white-space: pre-wrap;
  }

  .credits {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--line);
  }

  .switch {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }

  .switch input {
    flex: none;
    width: 20px;
    min-height: 20px;
    accent-color: var(--accent);
  }

  .small-button {
    align-self: flex-start;
    min-height: 36px;
    padding: 0 12px;
    font-size: 0.8125rem;
  }
</style>
