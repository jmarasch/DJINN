<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from './api'
  import { online, seen, systemName } from './nodeInfo'
  import Tabs from './Tabs.svelte'
  import { toast } from './state.svelte'
  import type { HouseNode, NodeList } from './types'

  let list = $state<NodeList | null>(null)
  let failed = $state(false)
  /** The request being accepted, and the password typed for it. */
  let accepting = $state<HouseNode | null>(null)
  let password = $state('')
  let kicking = $state<HouseNode | null>(null)
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

  /** Options first, then the nodes; opens on the nodes once there are some. */
  let tab = $state<'options' | 'nodes' | null>(null)
  const shown = $derived(tab ?? (list?.nodes.length ? 'nodes' : 'options'))

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

  /** Goes to the node's own page with a one-time link that lets this
   * browser manage it; its Exit node management button comes back here. */
  async function manage(node: HouseNode) {
    try {
      const { url } = await api.nodeManageLink(node.id)
      const back = `${location.origin}${location.pathname}#/admin/nodes`
      location.href = `${url}&back=${encodeURIComponent(back)}`
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not open it', true)
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

</script>

<Tabs
  wide
  items={[
    ['options', 'Options'],
    ['nodes', 'Nodes'],
  ]}
  active={shown}
  onselect={(id) => (tab = id as 'options' | 'nodes')}
/>

{#if shown === 'options'}
<section class="card wide">
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
{:else}
{#if waiting.length}
  <section class="card wide">
    <h3>Asking to join</h3>
    <p class="small muted">Check that the code matches the one on the node's screen before accepting.</p>
    {#each waiting as node (node.id)}
      <div class="node">
        <div class="grow">
          <b>{node.name}</b>
          <span class="small muted">
            Code <b class="code">{node.code}</b> · {node.address ?? 'unknown address'}{node.platform
              ? ` · ${systemName(node.platform)}`
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
  <section class="card wide">
    <h3>Joined</h3>
    {#if joined.length === 0}
      <p class="small muted">No nodes yet.</p>
    {:else}
      <p class="small muted">
        To line a node's speaker up with the others, use <a href="#/admin/rooms">Rooms</a> &gt; Timing on its room: play
        the timing sound, then move each speaker earlier or later until the tocks land together.
      </p>
    {/if}
    {#each joined as node (node.id)}
      <div class="joined-node">
        <div class="head">
          <span class="dot" class:on={online(node)} title={online(node) ? 'Connected' : 'Not connected'}></span>
          <b class="name">{node.name}</b>
          <span class="small muted">{online(node) ? 'Connected' : `Last seen ${seen(node.last_seen)}`}</span>
          {#if online(node)}
            {#if node.same_build}
              <span class="badge same">Same build as Main</span>
            {:else}
              <span class="badge behind">Not Main's build yet</span>
            {/if}
          {/if}
        </div>
        <p class="facts small muted">
          {#if node.addresses}<span>{node.addresses.split('\n').join(', ')}</span>{:else if node.address}<span>{node.address}</span>{/if}
          {#if node.platform}<span>{systemName(node.platform)}</span>{/if}
          {#if node.version}<span>{node.version}</span>{/if}
          {#if node.build_label}<span class="label">{node.build_label}</span>{/if}
        </p>
        <div class="actions">
          <button
            title={online(node)
              ? "Opens the node's own page, where its speaker, timing and other settings are"
              : "The node isn't connected; it can be managed once it is"}
            disabled={!online(node)}
            onclick={() => manage(node)}
          >
            Manage
          </button>
          {#if kicking?.id === node.id}
            <button class="danger" onclick={() => remove(node)}>Kick and wipe</button>
            <button onclick={() => (kicking = null)}>Cancel</button>
          {:else}
            <button class="danger" onclick={() => (kicking = node)}>Kick</button>
          {/if}
        </div>
      </div>
      {#if kicking?.id === node.id}
        <p class="small muted">
          {node.name} is shut out at once and wipes every cached song, now if it's connected or when it next starts.
        </p>
      {/if}
    {/each}
  </section>
{/if}
{/if}

<style>
  .node {
    display: flex;
    flex-wrap: wrap;
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

  /* A joined node: its name and state, what it runs, then its buttons. */
  .joined-node {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
  }

  .name {
    font-size: 1.05rem;
  }

  .badge {
    padding: 1px 9px;
    border-radius: 999px;
    border: 1px solid currentColor;
    font-size: 0.78rem;
  }

  .same {
    color: var(--ok);
  }

  .behind {
    color: var(--bad);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 14px;
    margin: 0;
  }

  .facts .label {
    color: var(--text);
    font-weight: 600;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
