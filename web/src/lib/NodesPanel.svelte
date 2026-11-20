<script lang="ts">
  import { t } from './i18n.svelte'
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
      toast(open ? t('np.can_join') : t('np.join_off'))
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
  }

  async function accept(e: SubmitEvent) {
    e.preventDefault()
    if (!accepting) return
    try {
      await api.acceptNode(accepting.id, password)
      toast(t('np.joined', { name: accepting.name }))
      accepting = null
      password = ''
      await load()
    } catch (err) {
      toast(err instanceof Error ? err.message : t('np.accept_failed'), true)
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
      toast(e instanceof Error ? e.message : t('ed.open_failed'), true)
    }
  }

  async function remove(node: HouseNode) {
    try {
      await api.removeNode(node.id)
      toast(node.status === 'pending' ? t('np.declined', { name: node.name }) : t('ns.kicked', { name: node.name }))
      kicking = null
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.remove_failed_short'), true)
    }
  }

</script>

<Tabs
  wide
  items={[
    ['options', t('np.options')],
    ['nodes', t('np.nodes')],
  ]}
  active={shown}
  onselect={(id) => (tab = id as 'options' | 'nodes')}
/>

{#if shown === 'options'}
<section class="card wide">
  <h2>{t('np.joining')}</h2>
  <p class="small muted">
    {t('np.intro')}
  </p>
  {#if failed}
    <p class="error">{t('np.load_failed')}</p>
  {:else if list}
    <label class="switch big">
      <input type="checkbox" checked={list.join_open} onchange={(e) => setJoining(e.currentTarget.checked)} />
      <b>{t('np.allow')}</b>
    </label>
    <p class="small muted">
      {t('np.allow_note')}
      {#if list.security_code}
        {t('np.code_1')} <b class="code">{list.security_code}</b>{t('np.code_2')}
      {/if}
    </p>
  {/if}
</section>
{:else}
{#if waiting.length}
  <section class="card wide">
    <h3>{t('np.asking')}</h3>
    <p class="small muted">{t('np.check_code')}</p>
    {#each waiting as node (node.id)}
      <div class="node">
        <div class="grow">
          <b>{node.name}</b>
          <span class="small muted">
            {t('np.code')} <b class="code">{node.code}</b> · {node.address ?? t('np.unknown_address')}{node.platform
              ? ` · ${systemName(node.platform)}`
              : ''}{t('np.asked', { when: seen(node.requested_at) })}
          </span>
        </div>
        <button class="primary" onclick={() => ((accepting = node), (password = ''))}>{t('np.accept')}</button>
        <button onclick={() => remove(node)}>{t('np.decline')}</button>
      </div>
      {#if accepting?.id === node.id}
        <form class="accept" onsubmit={accept}>
          <label>
            {t('np.password', { name: node.name })}
            <!-- svelte-ignore a11y_autofocus -->
            <input type="password" bind:value={password} autocomplete="current-password" autofocus required />
          </label>
          <div class="buttons">
            <button class="primary" type="submit">{t('np.accept_named', { name: node.name })}</button>
            <button type="button" onclick={() => (accepting = null)}>{t('common.cancel')}</button>
          </div>
        </form>
      {/if}
    {/each}
  </section>
{/if}

{#if list}
  <section class="card wide">
    <h3>{t('np.joined_title')}</h3>
    {#if joined.length === 0}
      <p class="small muted">{t('np.none')}</p>
    {:else}
      <p class="small muted">
        {t('np.timing_1')} <a href="#/admin/rooms">{t('admin.menu.rooms')}</a> {t('np.timing_2')}
      </p>
    {/if}
    {#each joined as node (node.id)}
      <div class="joined-node">
        <div class="head">
          <span class="dot" class:on={online(node)} title={online(node) ? t('ns.connected') : t('np.not_connected')}></span>
          <b class="name">{node.name}</b>
          <span class="small muted">{online(node) ? t('ns.connected') : t('ns.last_seen', { when: seen(node.last_seen) })}</span>
          {#if online(node)}
            {#if node.same_build}
              <span class="badge same">{t('ns.same_build')}</span>
            {:else}
              <span class="badge behind">{t('ns.behind')}</span>
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
              ? t('np.manage_title')
              : t('ns.offline_title')}
            disabled={!online(node)}
            onclick={() => manage(node)}
          >
            {t('np.manage')}
          </button>
          {#if kicking?.id === node.id}
            <button class="danger" onclick={() => remove(node)}>{t('ns.kick_wipe')}</button>
            <button onclick={() => (kicking = null)}>{t('common.cancel')}</button>
          {:else}
            <button class="danger" onclick={() => (kicking = node)}>{t('ns.kick')}</button>
          {/if}
        </div>
      </div>
      {#if kicking?.id === node.id}
        <p class="small muted">
          {t('ns.kick_note', { name: node.name })}
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
