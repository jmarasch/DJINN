<script lang="ts">
  // Admin > Access > Protected areas: how strict the lock on library changes
  // is, whether this session has it open, and the audit trail.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { AuditEntry, ProtectedLevel, ProtectedStatus } from './types'

  const levels: { id: ProtectedLevel; label: string; about: string }[] = [
    { id: 'off', label: 'Off', about: 'No lock. Anyone signed in as an admin can change or delete your files.' },
    { id: 'medium', label: 'Medium', about: 'The admin\'s password opens the protected areas.' },
    { id: 'max', label: 'Max', about: 'The admin\'s password and a two-step code. The recommended level.' },
  ]

  let status = $state<ProtectedStatus | null>(null)
  let entries = $state<AuditEntry[]>([])
  let total = $state(0)
  let page = $state(0)

  // Changing the level.
  let target = $state<ProtectedLevel | null>(null)
  let password = $state('')
  let code = $state('')
  let word = $state('')
  let busy = $state(false)

  const n = (x: number) => x.toLocaleString()

  async function refresh() {
    try {
      status = await api.protectedStatus()
      if (target === null) target = status.level
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load', true)
    }
  }

  async function loadAudit(to = page) {
    try {
      const found = await api.auditPage(Math.max(0, to) * 50)
      entries = found.entries
      total = found.total
      page = to
    } catch {
      // Shown next time.
    }
  }

  onMount(() => {
    void refresh()
    void loadAudit(0)
    const timer = setInterval(() => void refresh(), 10_000)
    return () => clearInterval(timer)
  })

  const needCode = $derived(status?.level === 'max' && status.two_step)
  const changing = $derived(status !== null && target !== null && target !== status.level)

  async function apply() {
    if (!target || !status) return
    busy = true
    try {
      await api.setProtectedLevel(target, password, code, word)
      toast('Saved')
      password = code = word = ''
      target = null
      await refresh()
      await loadAudit(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change it', true)
      code = ''
    } finally {
      busy = false
    }
  }

  async function lockNow() {
    await api.protectedLeave().catch(() => {})
    await refresh()
    await loadAudit(0)
  }

  async function unlock() {
    if (await askToUnlock()) {
      await refresh()
      await loadAudit(0)
    }
  }

  async function toggleAudit(on: boolean) {
    try {
      await unlocked(() => api.setAudit(on))
      await refresh()
      await loadAudit(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change it', true)
      await refresh()
    }
  }

  async function purge() {
    if (!confirm(`Delete all ${n(status?.audit_count ?? total)} audit records? This can't be undone.`)) return
    try {
      const done = await unlocked(() => api.purgeAudit())
      toast(`${n(done.removed)} records deleted`)
      await refresh()
      await loadAudit(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not delete them', true)
    }
  }

  const kinds: Record<string, string> = {
    entered: 'Unlocked',
    entry_failed: 'Unlock failed',
    left: 'Locked',
    level_changed: 'Level changed',
    audit_on: 'Audit trail turned on',
    audit_off: 'Audit trail turned off',
    audit_purged: 'Audit trail purged',
    approved_submission: 'Song added to the library',
  }
  const when = (ms: number) => new Date(ms).toLocaleString()
</script>

<section class="card">
  <h3>Protected areas</h3>
  <p class="small muted">
    Anything that changes your music library (the folders and the files in them) sits behind this lock: writing tags,
    renaming, moving, adding songs. Opening such an area asks for your password, and at Max a code from your
    authenticator app too. It locks again when you leave Admin, or after ten minutes without use. DJINN's own database
    (tag corrections kept in DJINN, history, lyrics) is not behind it.
  </p>
  {#if status}
    <p class="small">
      {#if status.level === 'off'}
        The lock is off: protected actions are open to every admin.
      {:else if status.open}
        Open for this session{#if status.seconds_left !== null}, locks in {Math.ceil(status.seconds_left / 60)} min without use{/if}.
        <button class="link" onclick={lockNow}>Lock now</button>
      {:else}
        Locked. <button class="link" onclick={unlock}>Unlock</button>
      {/if}
    </p>

    <h4>How strict</h4>
    <div class="levels" role="radiogroup" aria-label="How strict the lock is">
      {#each levels as l (l.id)}
        <label class="level" class:on={target === l.id}>
          <input type="radio" name="level" value={l.id} bind:group={target} />
          <strong>{l.label}</strong>
          <span class="small muted">{l.about}</span>
          {#if status.level === l.id}<span class="small tag">Now</span>{/if}
        </label>
      {/each}
    </div>
    {#if target === 'max' && !status.two_step}
      <p class="small bad">Max needs two-step sign-in on your account. Set it up on your Account page first.</p>
    {/if}
    {#if changing}
      <form
        class="change"
        onsubmit={(e) => {
          e.preventDefault()
          void apply()
        }}
      >
        {#if target === 'off'}
          <p class="small bad">
            With the lock off, anyone who can sign in as an admin can change or delete your files and really mess things
            up for you. That includes someone at an unlocked computer, a stolen session or a guessed password.
          </p>
          <label>
            Type OFF to turn the lock off
            <input bind:value={word} autocomplete="off" />
          </label>
        {/if}
        <label>
          Your password
          <input type="password" autocomplete="current-password" bind:value={password} />
        </label>
        {#if needCode}
          <label>
            Code (leaving Max needs one too)
            <input inputmode="numeric" autocomplete="one-time-code" bind:value={code} />
          </label>
        {/if}
        <div class="actions">
          <button class="primary" disabled={busy || !password || (target === 'off' && word.trim() !== 'OFF')}>
            Change to {levels.find((l) => l.id === target)?.label}
          </button>
          <button type="button" onclick={() => (target = status?.level ?? null)}>Cancel</button>
        </div>
      </form>
    {/if}
  {:else}
    <p class="small muted">Loading...</p>
  {/if}
</section>

<section class="card">
  <h3>Audit trail</h3>
  <p class="small muted">
    A record of each unlock, failed try, change of level and library change made through a protected area: who, when,
    and from where. You can turn it off and delete it. Turning it off or deleting records needs the area unlocked, and
    while the trail is on, a purge leaves one record of itself. Turn it off first for no trace.
  </p>
  {#if status}
    <label class="switch">
      <input type="checkbox" checked={status.audit_on} onchange={(e) => toggleAudit(e.currentTarget.checked)} />
      Keep an audit trail
    </label>
    <p class="small">
      {n(status.audit_count)} {status.audit_count === 1 ? 'record' : 'records'}.
      {#if status.audit_count > 0}<button class="danger small-button" onclick={purge}>Delete all records</button>{/if}
    </p>
  {/if}
  {#if entries.length}
    <ul class="trail">
      {#each entries as e (e.id)}
        <li>
          <span class="small muted">{when(e.at)}</span>
          <strong>{kinds[e.kind] ?? e.kind}</strong>
          <span class="small">{e.username}{e.ip ? ` from ${e.ip}` : ''}</span>
          {#if e.detail}<span class="small muted">{e.detail}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if total > 50}
      <div class="actions">
        <button class="small-button" disabled={page === 0} onclick={() => loadAudit(page - 1)}>Newer</button>
        <span class="small muted">Page {page + 1} of {Math.ceil(total / 50)}</span>
        <button class="small-button" disabled={(page + 1) * 50 >= total} onclick={() => loadAudit(page + 1)}>Older</button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .levels {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 8px 0;
  }

  .level {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 2px 10px;
    align-items: center;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
    cursor: pointer;
  }

  .level.on {
    border-color: var(--accent);
  }

  .level input {
    grid-row: 1 / span 2;
  }

  .level span:nth-of-type(1) {
    grid-column: 2;
  }

  .tag {
    grid-row: 1;
    grid-column: 3;
  }

  .change {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .trail {
    list-style: none;
    margin: 8px 0;
    padding: 0;
  }

  .trail li {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }

  .bad {
    color: var(--danger, #d33);
  }
</style>
