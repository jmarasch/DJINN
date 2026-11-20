<script lang="ts">
  // A node's own page: where it stands with its main DJINN. Before joining,
  // enter Main's address; then compare the codes and accept it on Main.
  // Once joined this page only shows status; leaving is Main's Kick.
  import { locale, t } from '../lib/i18n.svelte'
  import ScaleWarning from '../lib/ScaleWarning.svelte'
  import { onMount } from 'svelte'
  import NodeDiagnostics from '../lib/NodeDiagnostics.svelte'
  import NodeSpeakerSettings from '../lib/NodeSpeakerSettings.svelte'
  import NodeStage from '../lib/NodeStage.svelte'
  import type { NodeAudio, NodeSpeaker } from '../lib/types'
  import type { Visuals } from '../lib/visuals'

  interface NodeStatus {
    pairing: 'setup' | 'waiting' | 'joined' | 'declined' | 'kicked' | 'changed'
    name: string
    /** The DJINN it runs, and a test build's label (empty for a release). */
    version?: string
    build?: string
    main: string | null
    main_name: string | null
    /** Main's page for this node's screen (TV or lyrics), once joined. */
    screen?: string | null
    /** The visualizers Main says this screen draws. */
    visuals?: Visuals | null
    /** Main DJINNs heard on this network letting nodes join. */
    mains?: { name: string; address: string; joining: boolean }[]
    audio?: { screen?: string } | null
    /** What its screen shows: its own choice, else its room's. */
    shows?: string | null
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
    /** Stopped from the tray icon. */
    stopped?: boolean
  }

  let status = $state<NodeStatus | null>(null)
  /** Main's page this screen shows, once joined. */
  let stageUrl = $state<string | null>(null)
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
    if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? t('np2.save_failed'))
  }

  // Once joined, the node's page opens on its display (what its room or its
  // own choice says): from its tray icon, its address, or the kiosk (which
  // asks with ?screen=1). ?status=1 (the tray's Node status, and a manage
  // link from the main DJINN) asks for the status and settings instead.
  const startParams = new URLSearchParams(location.search)
  const kiosk = startParams.has('screen')
  const askedStatus = startParams.has('status') || startParams.has('manage') || startParams.has('back')
  const onScreen = kiosk || (!askedStatus && !startParams.has('code'))
  /** The tray icon's Show code: the party's join QR for this node's room. */
  const showCode = new URLSearchParams(location.search).has('code')
  let join = $state<{ url: string; room: string | null } | null>(null)
  let joinProblem = $state('')

  async function loadJoin() {
    const res = await fetch('/api/node/join').catch(() => null)
    if (res?.ok) {
      join = await res.json()
      joinProblem = ''
    } else {
      joinProblem = (await res?.json().catch(() => null))?.error ?? t('np2.no_main')
    }
  }
  /** Let in to manage this node (a link from the main DJINN's Admin > Nodes > Manage). */
  let managing = $state(false)

  /** Opens a manage link if this page came with one, then asks whether this
   * browser may manage the node. */
  async function checkManaging() {
    const params = new URLSearchParams(location.search)
    const token = params.get('manage')
    const back = params.get('back')
    if (back && /^https?:\/\//.test(back)) sessionStorage.setItem(BACK_KEY, back)
    if (token || back) {
      params.delete('manage')
      params.delete('back')
      // A reload stays on the status page.
      params.set('status', '1')
      const rest = params.toString()
      history.replaceState(null, '', location.pathname + (rest ? `?${rest}` : ''))
    }
    if (token) {
      const res = await fetch('/api/node/manage', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ token }),
      }).catch(() => null)
      if (res && !res.ok) error = (await res.json().catch(() => null))?.error ?? t('np2.bad_link')
    }
    const res = await fetch('/api/node/manage').catch(() => null)
    managing = !!res?.ok && !!(await res.json()).managing
  }

  /** Where Exit node management goes: the main DJINN's Admin > Nodes. */
  const BACK_KEY = 'djinn-node-back'

  async function exitManaging() {
    await fetch('/api/node/manage', { method: 'DELETE' }).catch(() => null)
    managing = false
    const back = sessionStorage.getItem(BACK_KEY)
    sessionStorage.removeItem(BACK_KEY)
    if (back) location.href = back
  }

  async function load() {
    try {
      const res = await fetch('/api/node/status')
      if (!res.ok) throw new Error()
      status = await res.json()
      unreachable = false
      // On the box's own screen (the kiosk opens /screen): Main's page for
      // it once joined, in a frame over this node's visualizer (`NodeStage`).
      stageUrl = onScreen && status?.pairing === 'joined' && status.screen ? status.screen : null
      // A display set to Off is a blank screen on the kiosk; opened from the
      // tray or the node's address, the status page is more use than black.
      if (!name && status) name = status.name
    } catch {
      unreachable = true
    }
  }

  onMount(() => {
    document.title = 'DJINN node'
    void load()
    if (showCode) void loadJoin()
    void checkManaging()
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
      if (!res.ok) throw new Error(data?.error ?? t('common.failed'))
      status = data
      editing = false
    } catch (e) {
      error = e instanceof Error ? e.message : t('common.failed')
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

{#if kiosk && status?.pairing === 'joined' && status.shows === 'off'}
  <div class="blank"></div>
{:else if stageUrl}
  {#key stageUrl}<NodeStage url={stageUrl} visuals={status?.visuals ?? null} />{/key}
{/if}

{#if showCode}
  <main class="narrow">
    <div class="card join">
      <h1>{t('join.join')}</h1>
      {#if join}
        {#if join.room}<p class="muted">{t('np2.in_room', { room: join.room })}</p>{/if}
        <img class="qr" src="/api/node/join/qr.svg?v={encodeURIComponent(join.url)}" alt={t('ns2.qr', { url: join.url })} />
        <p class="small">{join.url}</p>
      {:else if joinProblem}
        <p class="error">{joinProblem}</p>
        <button onclick={loadJoin}>{t('common.try_again')}</button>
      {:else}
        <p class="small muted"><span class="spinner" aria-hidden="true"></span> {t('np2.asking')}</p>
      {/if}
    </div>
  </main>
{:else}
<main class="narrow" class:wide={managing}>
  {#if managing}
    <div class="managing">
      <span>{t('np2.managing', { name: status?.name ?? t('np2.this_node') })}</span>
      <button class="primary" onclick={exitManaging}>{t('np2.exit')}</button>
    </div>
  {/if}
  <div class="card">
    {#if status?.pairing !== 'joined'}<img class="waiting" src="/images/waiting-for-main.webp" alt="" />{/if}
    <h1>{t('np2.title')}</h1>
    {#if unreachable && !status}
      <p class="muted">{t('np2.unreachable')}</p>
    {:else if status}
      {#if showForm}
        <p class="muted">
          {t('np2.intro')}
        </p>
        {#if status.mains?.length}
          <div class="mains">
            {#each status.mains as main (main.address)}
              <button type="button" class:primary={address === main.address} onclick={() => (address = main.address)}>
                <b>{main.name}</b> <span class="small">{main.address}</span>
                {#if !main.joining}<br /><span class="small">{t('np2.not_joining')}</span>{/if}
              </button>
            {/each}
          </div>
        {:else}
          <p class="small muted"><span class="spinner" aria-hidden="true"></span> {t('np2.looking')}</p>
        {/if}
        <form onsubmit={connect}>
          <label>
            {t('np2.address')}
            <input bind:value={address} placeholder="192.168.1.10:8066" autocapitalize="none" required />
          </label>
          <label>
            {t('np2.name')}
            <input bind:value={name} placeholder={t('np2.name_hint')} maxlength="60" />
          </label>
          {#if error}<p class="error">{error}</p>{/if}
          <div class="buttons">
            <button class="primary" disabled={busy}>{t('np2.ask')}</button>
            {#if editing}<button type="button" onclick={() => (editing = false)}>{t('common.cancel')}</button>{/if}
          </div>
        </form>
      {:else}
        <p class="state">
          {#if status.pairing === 'waiting'}
            <span class="spinner" aria-hidden="true"></span> {t('np2.waiting', { main: status.main_name ?? t('np2.the_main') })}
          {:else if status.pairing === 'joined'}
            <span class="dot on"></span> {t('np2.joined', { main: status.main_name ?? t('np2.the_main') })}
          {:else if status.pairing === 'declined'}
            {t('np2.declined')}
          {:else if status.pairing === 'kicked'}
            {t('np2.kicked')}
          {:else if status.pairing === 'changed'}
            {t('np2.changed')}
          {/if}
        </p>
        {#if status.problem}<p class="error">{status.problem}</p>{/if}
        {#if status.pairing === 'joined'}
          <p class="small">
            {#if status.stopped}
              {t('np2.stopped')}
            {:else if status.offline}
              {t('np2.offline')}
            {:else if status.playing}
              {t('np2.playing')}
            {:else if status.sync_ms == null}
              {t('np2.matching')}
            {:else}
              {t('np2.not_playing')}
            {/if}
            {#if status.audio_problem && !status.playing}<span class="error"> {status.audio_problem}</span>{/if}
          </p>
        {/if}

        {#if status.pairing === 'waiting'}
          <p class="muted">
            {t('np2.accept_note')}
          </p>
        {/if}
        <dl>
          <dt>{t('np2.node')}</dt>
          <dd>{status.name}</dd>
          <dt>{t('np2.main')}</dt>
          <dd>{status.main ?? t('np2.not_set')}</dd>
          {#if status.main_code}
            <dt>{t('np2.main_code')}</dt>
            <dd class="code">{status.main_code}</dd>
          {/if}
          {#if status.version}
            <dt>{t('np2.version')}</dt>
            <dd>{status.version}{#if status.build}<br /><span class="small muted">{status.build}</span>{/if}</dd>
          {/if}
          {#if status.pairing === 'joined'}
            <dt>{t('np2.cached')}</dt>
            <dd>{status.cached}</dd>
          {/if}
          {#if status.pairing === 'joined' && status.sync_ms != null}
            <dt>{t('np2.clock')}</dt>
            <dd>{t('np2.within', { ms: status.sync_ms.toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}</dd>
          {/if}
          {#if status.pairing === 'joined' && status.playing && status.speaker_ms != null}
            <dt>{t('np2.speaker')}</dt>
            <dd>
              {Math.abs(status.speaker_ms) < 0.5
                ? t('np2.on_time')
                : t(status.speaker_ms > 0 ? 'np2.behind' : 'np2.ahead', {
                    ms: Math.abs(status.speaker_ms).toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
                  })}
            </dd>
          {/if}
          {#if status.code && status.pairing === 'waiting'}
            <dt>{t('np2.request_code')}</dt>
            <dd class="code big">{status.code}</dd>
          {/if}
        </dl>
        {#if status.main_code && status.pairing === 'waiting'}
          <p class="small muted">
            {t('np2.code_note')}
          </p>
        {/if}
        {#if error}<p class="error">{error}</p>{/if}
        {#if status.pairing === 'joined' && status.screen}
          <p><a href="/node">{t('np2.show_display')}</a></p>
        {/if}
        {#if status.pairing === 'joined' && !managing}
          <p class="small muted">
            {t('np2.manage_note')}
          </p>
        {/if}
        {#if status.pairing === 'joined' && managing}
          <button onclick={openSpeaker}>{speakerOpen ? t('np2.hide_speaker') : t('np2.speaker_settings')}</button>
          {#if speakerOpen && speakerView?.audio}
            <NodeSpeakerSettings audio={speakerView.audio} speakers={speakerView.speakers} save={saveAudio} />
          {:else if speakerOpen && speakerView}
            <p class="small muted">{t('np2.arriving')}</p>
          {/if}
        {/if}
        <div class="buttons">
          {#if status.pairing === 'waiting'}
            <button disabled={busy} onclick={() => ((editing = true), (address = status?.main ?? ''))}>
              {t('np2.change_address')}
            </button>
          {:else if status.pairing === 'declined' || status.pairing === 'kicked' || status.pairing === 'changed'}
            <button class="primary" disabled={busy} onclick={() => send('/api/node/ask-again')}>{t('np2.ask_again')}</button>
            <button disabled={busy} onclick={() => ((editing = true), (address = status?.main ?? ''))}>
              {t('np2.another')}
            </button>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
  {#if managing && status?.pairing === 'joined'}
    <NodeDiagnostics />
  {/if}
</main>
{/if}

{#if onScreen}<ScaleWarning />{/if}

<style>
  .join {
    align-items: center;
    text-align: center;
  }

  .qr {
    width: min(320px, 80vw);
    height: auto;
    background: #fff;
    padding: 12px;
    border-radius: 12px;
  }

  .waiting {
    align-self: center;
    width: min(180px, 50vw);
    height: auto;
  }

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

  main.narrow.wide {
    max-width: 820px;
    padding-top: 16px;
  }

  .managing {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 12px;
    padding: 8px 12px;
    border-radius: 12px;
    background: var(--surface-2);
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
