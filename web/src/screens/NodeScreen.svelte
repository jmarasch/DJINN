<script lang="ts">
  // A node's own page: where it stands with its main DJINN. Before joining,
  // enter Main's address; then compare the codes and accept it on Main.
  // Once joined this page only shows status; leaving is Main's Kick.
  import { onMount } from 'svelte'
  import NodeSpeakerSettings from '../lib/NodeSpeakerSettings.svelte'
  import type { NodeAudio, NodeSpeaker } from '../lib/types'

  interface NodeStatus {
    pairing: 'setup' | 'waiting' | 'joined' | 'declined' | 'kicked' | 'changed'
    name: string
    main: string | null
    main_name: string | null
    /** Main's TV page for this node's room, once joined. */
    screen?: string | null
    /** Main DJINNs heard on this network letting nodes join. */
    mains?: { name: string; address: string; joining: boolean }[]
    audio?: { screen?: string } | null
    main_code: string | null
    code: string | null
    room_id: number | null
    problem: string | null
    playing: boolean
    sync_ms: number | null
    audio_problem: string | null
    speaker_ms: number | null
    offline: boolean
    cached: number
  }

  let status = $state<NodeStatus | null>(null)
  let unreachable = $state(false)
  let address = $state('')
  let name = $state('')
  let error = $state('')
  let busy = $state(false)
  let editing = $state(false)
  let speakerOpen = $state(false)
  let speakerView = $state<{ audio: NodeAudio | null; speakers: NodeSpeaker[] } | null>(null)

  async function openSpeaker() {
    speakerOpen = !speakerOpen
    if (!speakerOpen) return
    const res = await fetch('/api/node/audio')
    speakerView = res.ok ? await res.json() : null
  }

  async function saveAudio(audio: NodeAudio) {
    const res = await fetch('/api/node/audio', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(audio),
    })
    if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? 'Could not save')
  }

  const onScreen = new URLSearchParams(location.search).has('screen')

  async function load() {
    try {
      const res = await fetch('/api/node/status')
      if (!res.ok) throw new Error()
      status = await res.json()
      unreachable = false
      // On the box's own screen (the kiosk opens /screen): show Main's TV
      // page once joined.
      if (onScreen && status?.pairing === 'joined' && status.screen) location.replace(status.screen)
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

{#if onScreen && status?.pairing === 'joined' && status.audio?.screen === 'off'}
  <div class="blank"></div>
{/if}

<main class="narrow">
  <div class="card">
    <h1>DJINN node</h1>
    {#if unreachable && !status}
      <p class="muted">Can't reach this node's DJINN. Is it running?</p>
    {:else if status}
      {#if showForm}
        <p class="muted">
          This computer plays one room's music for a main DJINN. Pick yours below, and turn on Admin &gt; Nodes &gt;
          "Allow nodes to ask to join" there before asking.
        </p>
        {#if status.mains?.length}
          <div class="mains">
            {#each status.mains as main (main.address)}
              <button type="button" class:primary={address === main.address} onclick={() => (address = main.address)}>
                <b>{main.name}</b> <span class="small">{main.address}</span>
                {#if !main.joining}<br /><span class="small">Not letting nodes join right now</span>{/if}
              </button>
            {/each}
          </div>
        {:else}
          <p class="small muted"><span class="spinner" aria-hidden="true"></span> Looking for main DJINNs on this network...</p>
        {/if}
        <form onsubmit={connect}>
          <label>
            Main DJINN's address (pick one above, or type it)
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
        {#if status.pairing === 'joined'}
          <p class="small">
            {#if status.offline}
              The main DJINN can't be reached. Playing cached songs until it's back (or quiet, if this node is set to
              stop).
            {:else if status.playing}
              Playing Main's music in step.
            {:else if status.sync_ms == null}
              Matching clocks with Main...
            {:else}
              Not playing right now.
            {/if}
            {#if status.audio_problem && !status.playing}<span class="error"> {status.audio_problem}</span>{/if}
          </p>
        {/if}

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
          {#if status.pairing === 'joined'}
            <dt>Cached songs</dt>
            <dd>{status.cached}</dd>
          {/if}
          {#if status.pairing === 'joined' && status.sync_ms != null}
            <dt>Clock matched to</dt>
            <dd>within {status.sync_ms.toFixed(1)} ms</dd>
          {/if}
          {#if status.pairing === 'joined' && status.playing && status.speaker_ms != null}
            <dt>Speaker timing</dt>
            <dd>
              {Math.abs(status.speaker_ms) < 0.5
                ? 'on time'
                : `${Math.abs(status.speaker_ms).toFixed(1)} ms ${status.speaker_ms > 0 ? 'behind' : 'ahead of'} Main`}
            </dd>
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
        {#if status.pairing === 'joined'}
          <button onclick={openSpeaker}>{speakerOpen ? 'Hide speaker settings' : 'Speaker settings'}</button>
          {#if speakerOpen && speakerView?.audio}
            <NodeSpeakerSettings audio={speakerView.audio} speakers={speakerView.speakers} save={saveAudio} />
          {:else if speakerOpen && speakerView}
            <p class="small muted">Settings arrive from the main DJINN in a moment.</p>
          {/if}
        {/if}
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
  .blank {
    position: fixed;
    inset: 0;
    z-index: 10;
    background: #000;
    cursor: none;
  }

  .mains {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 12px;
  }

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
