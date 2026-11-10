<script lang="ts">
  // Admin > Metrics and logs > Sign-in log: every sign-in to DJINN and DJINN
  // GO, a page at a time, with clearing old ones.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { deviceName } from './format'
  import { toast } from './state.svelte'
  import type { SignIn } from './types'

  const PAGE = 50
  let items = $state<SignIn[]>([])
  let total = $state(0)
  let page = $state(0)
  let failed = $state(false)
  let keepDays = $state(30)
  let confirming = $state(false)

  const pages = $derived(Math.max(1, Math.ceil(total / PAGE)))

  async function load() {
    try {
      const got = await api.signIns(page * PAGE, PAGE)
      items = got.items
      total = got.total
      failed = false
      if (page > 0 && page >= Math.ceil(total / PAGE)) {
        page = Math.max(0, Math.ceil(total / PAGE) - 1)
        await load()
      }
    } catch {
      failed = true
    }
  }

  onMount(() => void load())

  function go(to: number) {
    page = Math.min(Math.max(0, to), pages - 1)
    void load()
  }

  async function clear() {
    try {
      const { cleared } = await api.clearSignIns(keepDays)
      toast(cleared === 1 ? 'Cleared 1 sign-in' : `Cleared ${cleared.toLocaleString()} sign-ins`)
      confirming = false
      page = 0
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not clear them', true)
    }
  }

  const outcomes: Record<string, string> = {
    ok: 'Signed in',
    wrong_password: 'Wrong password',
    locked_out: 'Locked out (too many tries)',
    waiting: 'Waiting for approval',
    turned_off: 'Account turned off',
    wrong_code: 'Wrong two-step code',
    no_go: "Role can't use GO",
    not_home: 'Away from home, not allowed',
  }
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

<section class="card">
  <h2>Sign-in log</h2>
  <p class="small muted">
    Every sign-in to DJINN and DJINN GO, kept for 90 days at most. Five wrong passwords lock an account for 15 minutes.
  </p>
  {#if failed}
    <p class="error">Could not load the sign-ins.</p>
  {:else}
    {#each items as s (s.id)}
      <div class="row" class:bad={s.outcome !== 'ok'}>
        <div><b>{s.username}</b> {outcomes[s.outcome] ?? s.outcome} <span class="muted small">{s.via === 'go' ? 'GO' : 'party'}</span></div>
        <div class="muted small">{when(s.at)}, {s.ip ?? 'unknown address'}, {deviceName(s.user_agent)}</div>
      </div>
    {:else}
      <p class="muted">No sign-ins yet.</p>
    {/each}
    {#if total > PAGE}
      <div class="pager">
        <button class="small-button" onclick={() => go(page - 1)} disabled={page === 0}>Newer</button>
        <span class="small muted">Page {page + 1} of {pages} ({total.toLocaleString()} sign-ins)</span>
        <button class="small-button" onclick={() => go(page + 1)} disabled={page >= pages - 1}>Older</button>
      </div>
    {/if}
  {/if}
</section>

<section class="card">
  <h3>Clear old sign-ins</h3>
  <label class="keep">
    Keep the last
    <select bind:value={keepDays}>
      <option value={1}>day</option>
      <option value={7}>7 days</option>
      <option value={30}>30 days</option>
      <option value={0}>nothing (clear all)</option>
    </select>
  </label>
  {#if confirming}
    <p class="small">
      {keepDays === 0 ? 'Every sign-in is removed from the log.' : `Sign-ins older than ${keepDays === 1 ? 'a day' : `${keepDays} days`} are removed.`}
      Lockouts and signed-in devices aren't affected.
    </p>
    <button class="danger" onclick={clear}>Clear them</button>
    <button onclick={() => (confirming = false)}>Cancel</button>
  {:else}
    <button onclick={() => (confirming = true)}>Clear</button>
  {/if}
</section>

<style>
  .row {
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .row.bad b {
    color: var(--bad);
  }

  .pager {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: 12px;
  }

  .keep {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
</style>
