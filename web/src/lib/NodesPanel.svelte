<script lang="ts">
  // Admin > Nodes: other DJINN boxes that play a room on their own speakers.
  // Joining is off until switched on; each request is accepted with the
  // admin's password after checking its code matches the node's screen.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { HouseNode, NodeList } from './types'
  import NodeSpeakerSettings from './NodeSpeakerSettings.svelte'

  let list = $state<NodeList | null>(null)
  let failed = $state(false)
  /** The request being accepted, and the password typed for it. */
  let accepting = $state<HouseNode | null>(null)
  let password = $state('')
  let kicking = $state<HouseNode | null>(null)
  /** The node whose speaker settings are open. */
  let tuning = $state<number | null>(null)

  async function load() {
    try {
      list = await api.nodes()
      failed = false
    } catch {
      failed = true
    }
  }

  onMount(() => {
    void load()
    // Requests arrive while the page is open.
    const timer = setInterval(load, 5000)
    return () => clearInterval(timer)
  })

  const waiting = $derived(list?.nodes.filter((n) => n.status === 'pending') ?? [])
  const joined = $derived(list?.nodes.filter((n) => n.status === 'approved') ?? [])

  async function setJoining(open: boolean) {
    try {
      await api.setNodeJoining(open)
      toast(open ? 'Nodes can ask to join' : 'Joining is off')
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
  }

  async function accept(e: SubmitEvent) {
    e.preventDefault()
    if (!accepting) return
    try {
      await api.acceptNode(accepting.id, password)
      toast(`${accepting.name} joined`)
      accepting = null
      password = ''
      await load()
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not accept', true)
    }
  }

  async function remove(node: HouseNode) {
    try {
      await api.removeNode(node.id)
      toast(node.status === 'pending' ? `Declined ${node.name}` : `Kicked ${node.name}; it will wipe its cache`)
      kicking = null
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove', true)
    }
  }

  function seen(ms: number | null): string {
    if (!ms) return 'never'
    const minutes = Math.round((Date.now() - ms) / 60_000)
    if (minutes < 1) return 'just now'
    if (minutes < 60) return `${minutes} min ago`
    const hours = Math.round(minutes / 60)
    if (hours < 48) return `${hours} h ago`
    return new Date(ms).toLocaleDateString()
  }
  const online = (node: HouseNode) => !!node.last_seen && Date.now() - node.last_seen < 2 * 60_000
</script>

<section class="card">
  <h2>Joining</h2>
  <p class="small muted">
    A node is another DJINN box, like a Raspberry Pi by a room's speakers. It plays that room's music in step with this
    server and takes everything from here: songs are only cached on it, never kept. Phones always connect to this server.
  </p>
  {#if failed}
    <p class="error">Could not load the nodes.</p>
  {:else if list}
    <label class="switch big">
      <input type="checkbox" checked={list.join_open} onchange={(e) => setJoining(e.currentTarget.checked)} />
      <b>Allow nodes to ask to join</b>
    </label>
    <p class="small muted">
      Turn this on while setting up a node, and off again after. Requests still need your password to accept.
      {#if list.security_code}
        A node shows this server's code when it first connects: <b class="code">{list.security_code}</b>. If it shows a
        different code, it reached something else; don't go on.
      {/if}
    </p>
  {/if}
</section>

{#if waiting.length}
  <section class="card">
    <h3>Asking to join</h3>
    <p class="small muted">Check that the code matches the one on the node's screen before accepting.</p>
    {#each waiting as node (node.id)}
      <div class="node">
        <div class="grow">
          <b>{node.name}</b>
          <span class="small muted">
            Code <b class="code">{node.code}</b> · {node.address ?? 'unknown address'}{node.platform
              ? ` · ${node.platform}`
              : ''} · asked {seen(node.requested_at)}
          </span>
        </div>
        <button class="primary" onclick={() => ((accepting = node), (password = ''))}>Accept</button>
        <button onclick={() => remove(node)}>Decline</button>
      </div>
      {#if accepting?.id === node.id}
        <form class="accept" onsubmit={accept}>
          <label>
            Your password, to let {node.name} join
            <!-- svelte-ignore a11y_autofocus -->
            <input type="password" bind:value={password} autocomplete="current-password" autofocus required />
          </label>
          <div class="buttons">
            <button class="primary" type="submit">Accept {node.name}</button>
            <button type="button" onclick={() => (accepting = null)}>Cancel</button>
          </div>
        </form>
      {/if}
    {/each}
  </section>
{/if}

{#if list}
  <section class="card">
    <h3>Joined</h3>
    {#if joined.length === 0}
      <p class="small muted">No nodes yet.</p>
    {/if}
    {#each joined as node (node.id)}
      <div class="node">
        <span class="dot" class:on={online(node)} title={online(node) ? 'Connected' : 'Not connected'}></span>
        <div class="grow">
          <b>{node.name}</b>
          <span class="small muted">
            {online(node) ? 'Connected' : `Last seen ${seen(node.last_seen)}`}{node.address
              ? ` · ${node.address}`
              : ''}{node.version ? ` · ${node.version}` : ''}
          </span>
        </div>
        <button class:on={tuning === node.id} onclick={() => (tuning = tuning === node.id ? null : node.id)}>
          Speaker
        </button>
        {#if kicking?.id === node.id}
          <button class="danger" onclick={() => remove(node)}>Kick and wipe</button>
          <button onclick={() => (kicking = null)}>Cancel</button>
        {:else}
          <button onclick={() => (kicking = node)}>Kick</button>
        {/if}
      </div>
      {#if tuning === node.id}
        <NodeSpeakerSettings
          audio={node.audio}
          speakers={node.speakers}
          save={(audio) => api.setNodeAudio(node.id, audio)}
        />
      {/if}
      {#if kicking?.id === node.id}
        <p class="small muted">
          {node.name} is shut out at once and wipes every cached song, now if it's connected or when it next starts.
        </p>
      {/if}
    {/each}
  </section>
{/if}

<style>
  .node {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .node:last-of-type {
    border-bottom: none;
  }

  .grow {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .code {
    font-family: ui-monospace, monospace;
    letter-spacing: 0.05em;
  }

  .dot {
    width: 10px;
    height: 10px;
    flex: none;
    border-radius: 50%;
    background: var(--line);
  }

  .dot.on {
    background: var(--good, #3a9a5b);
  }

  .accept {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 4px 0 12px;
  }

  .accept label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .buttons {
    display: flex;
    gap: 8px;
  }
</style>
