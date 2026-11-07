<script lang="ts">
  // A node's own page: where it stands with its main DJINN. Before joining,
  // enter Main's address; then compare the codes and accept it on Main.
  // Once joined this page only shows status; leaving is Main's Kick.
  import { onMount } from 'svelte'

  interface NodeStatus {
    pairing: 'setup' | 'waiting' | 'joined' | 'declined' | 'kicked' | 'changed'
    name: string
    main: string | null
    main_name: string | null
    main_code: string | null
    code: string | null
    room_id: number | null
    problem: string | null
  }

  let status = $state<NodeStatus | null>(null)
  let unreachable = $state(false)
  let address = $state('')
  let name = $state('')
  let error = $state('')
  let busy = $state(false)
  let editing = $state(false)

  async function load() {
    try {
      const res = await fetch('/api/node/status')
      if (!res.ok) throw new Error()
      status = await res.json()
      unreachable = false
      if (!name && status) name = status.name
    } catch {
      unreachable = true
    }
  }

  onMount(() => {
    document.title = 'DJINN node'
    void load()
    const timer = setInterval(load, 3000)
    return () => clearInterval(timer)
  })

  async function send(path: string, body?: unknown) {
    busy = true
    error = ''
    try {
      const res = await fetch(path, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body ?? {}),
      })
      const data = await res.json().catch(() => null)
      if (!res.ok) throw new Error(data?.error ?? 'That did not work')
      status = data
      editing = false
    } catch (e) {
      error = e instanceof Error ? e.message : 'That did not work'
    } finally {
      busy = false
    }
  }

  function connect(e: SubmitEvent) {
    e.preventDefault()
    void send('/api/node/connect', { address, name })
  }

  const showForm = $derived(!!status && (status.pairing === 'setup' || editing))
</script>

<main class="narrow">
  <div class="card">
    <h1>DJINN node</h1>
    {#if unreachable && !status}
      <p class="muted">Can't reach this node's DJINN. Is it running?</p>
    {:else if status}
      {#if showForm}
        <p class="muted">
          This computer plays one room's music for a main DJINN. Enter the main DJINN's address (as shown in its
          browser bar, like 192.168.1.10:8066). Turn on Admin &gt; Nodes &gt; "Allow nodes to ask to join" there first.
        </p>
        <form onsubmit={connect}>
          <label>
            Main DJINN's address
            <input bind:value={address} placeholder="192.168.1.10:8066" autocapitalize="none" required />
          </label>
          <label>
            This node's name
            <input bind:value={name} placeholder="Kitchen" maxlength="60" />
          </label>
          {#if error}<p class="error">{error}</p>{/if}
          <div class="buttons">
            <button class="primary" disabled={busy}>Ask to join</button>
            {#if editing}<button type="button" onclick={() => (editing = false)}>Cancel</button>{/if}
          </div>
        </form>
      {:else}
        <p class="state">
          {#if status.pairing === 'waiting'}
            <span class="spinner" aria-hidden="true"></span> Waiting for {status.main_name ?? 'the main DJINN'} to accept
            this node
          {:else if status.pairing === 'joined'}
            <span class="dot on"></span> Joined {status.main_name ?? 'the main DJINN'}
          {:else if status.pairing === 'declined'}
            The main DJINN declined this node.
          {:else if status.pairing === 'kicked'}
            The main DJINN kicked this node. Every cached song was wiped.
          {:else if status.pairing === 'changed'}
            The main DJINN answered with a different identity than before.
          {/if}
        </p>
        {#if status.problem}<p class="error">{status.problem}</p>{/if}

        {#if status.pairing === 'waiting'}
          <p class="muted">
            On the main DJINN, open Admin &gt; Nodes. Check both codes match before accepting there with your password.
          </p>
        {/if}
        <dl>
          <dt>Node</dt>
          <dd>{status.name}</dd>
          <dt>Main DJINN</dt>
          <dd>{status.main ?? 'not set'}</dd>
          {#if status.main_code}
            <dt>Main's security code</dt>
            <dd class="code">{status.main_code}</dd>
          {/if}
          {#if status.code && status.pairing === 'waiting'}
            <dt>This request's code</dt>
            <dd class="code big">{status.code}</dd>
          {/if}
        </dl>
        {#if status.main_code && status.pairing === 'waiting'}
          <p class="small muted">
            Main's security code should match the one in Admin &gt; Nodes on the main DJINN. If it doesn't, this node
            reached something else: change the address.
          </p>
        {/if}
        {#if error}<p class="error">{error}</p>{/if}
        <div class="buttons">
          {#if status.pairing === 'waiting'}
            <button disabled={busy} onclick={() => ((editing = true), (address = status?.main ?? ''))}>
              Change address
            </button>
          {:else if status.pairing === 'declined' || status.pairing === 'kicked' || status.pairing === 'changed'}
            <button class="primary" disabled={busy} onclick={() => send('/api/node/ask-again')}>Ask to join again</button>
            <button disabled={busy} onclick={() => ((editing = true), (address = status?.main ?? ''))}>
              Use another main DJINN
            </button>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</main>

<style>
  main.narrow {
    max-width: 460px;
    margin: 0 auto;
    padding: 10vh 16px 32px;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 14px;
    margin: 14px 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .code {
    font-family: ui-monospace, monospace;
    letter-spacing: 0.05em;
  }

  .code.big {
    font-size: 1.4rem;
    font-weight: 700;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--line);
  }

  .dot.on {
    background: var(--good, #3a9a5b);
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
