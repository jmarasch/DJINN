<script lang="ts">
  // Everything about one node, in one place: what it is, its room, its
  // speaker and screen, AI lyrics help, its own page and kicking it out. The
  // Rooms page shows this in a window; the node's own page (its address)
  // stays as it is.
  import { t } from './i18n.svelte'
  import { api } from './api'
  import NodeAiSettings from './NodeAiSettings.svelte'
  import NodeSpeakerSettings from './NodeSpeakerSettings.svelte'
  import { online, seen, systemName } from './nodeInfo'
  import { toast } from './state.svelte'
  import type { HouseNode, Room } from './types'

  let {
    node,
    rooms,
    onchange,
    onclose,
  }: {
    node: HouseNode
    rooms: Room[]
    onchange: () => void | Promise<void>
    onclose: () => void
  } = $props()

  let kicking = $state(false)

  async function moveTo(room: number) {
    try {
      await api.setNodeRoom(node.id, room)
      toast(t('ns.moved', { name: node.name }))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('rm.move_failed'), true)
    }
    await onchange()
  }

  /** Goes to the node's own page with a one-time link that lets this
   * browser manage it; its Exit node management button comes back here. */
  async function manage() {
    try {
      const { url } = await api.nodeManageLink(node.id)
      const back = `${location.origin}${location.pathname}#/admin/rooms`
      location.href = `${url}&back=${encodeURIComponent(back)}`
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.open_failed'), true)
    }
  }

  async function kick() {
    try {
      await api.removeNode(node.id)
      toast(t('ns.kicked', { name: node.name }))
      onclose()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.remove_failed_short'), true)
    }
    await onchange()
  }
</script>

<section>
  <p class="state">
    <span class="dot" class:on={online(node)}></span>
    <b>{online(node) ? t('ns.connected') : t('ns.last_seen', { when: seen(node.last_seen) })}</b>
    {#if online(node)}
      {#if node.same_build}
        <span class="badge same">{t('ns.same_build')}</span>
      {:else}
        <span class="badge behind">{t('ns.behind')}</span>
      {/if}
    {/if}
  </p>
  <p class="facts small muted">
    {#if node.address}<span>{node.address}</span>{/if}
    {#if node.platform}<span>{systemName(node.platform)}</span>{/if}
    {#if node.version}<span>{node.version}</span>{/if}
    {#if node.build_label}<span class="label">{node.build_label}</span>{/if}
  </p>
  <label class="field">
    {t('rm.room')}
    <select value={node.room_id} onchange={(e) => moveTo(Number(e.currentTarget.value))}>
      {#each rooms as room (room.id)}<option value={room.id}>{room.name}</option>{/each}
    </select>
  </label>
</section>

<section>
  <h3>{t('ns.speaker_screen')}</h3>
  {#key node.id}
    <NodeSpeakerSettings
      audio={node.audio}
      speakers={node.speakers}
      save={async (audio) => {
        await api.setNodeAudio(node.id, audio)
        await onchange()
      }}
    />
  {/key}
</section>

<section>
  <h3>{t('ns.ai')}</h3>
  <NodeAiSettings {node} {onchange} />
</section>

<section>
  <h3>{t('ns.this_node')}</h3>
  <div class="actions">
    <button
      title={online(node)
        ? t('ns.own_page_title')
        : t('ns.offline_title')}
      disabled={!online(node)}
      onclick={manage}
    >
      {t('ns.own_page')}
    </button>
    {#if kicking}
      <button class="danger" onclick={kick}>{t('ns.kick_wipe')}</button>
      <button onclick={() => (kicking = false)}>{t('common.cancel')}</button>
    {:else}
      <button class="danger" onclick={() => (kicking = true)}>{t('ns.kick')}</button>
    {/if}
  </div>
  {#if kicking}
    <p class="small muted">
      {t('ns.kick_note', { name: node.name })}
    </p>
  {/if}
  <p class="small muted">{t('ns.port_note')}</p>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  h3 {
    margin: 0;
    font-size: 1rem;
  }

  p {
    margin: 0;
  }

  .state {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--line);
  }

  .dot.on {
    background: var(--ok);
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
  }

  .facts .label {
    color: var(--text);
    font-weight: 600;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 320px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
