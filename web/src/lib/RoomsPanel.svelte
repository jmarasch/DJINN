<script lang="ts">
  // Admin > Rooms: areas with their own speakers, each its own party (queue,
  // rules, skip votes) unless linked to play along with another. A card per
  // room: four groups of settings (display, what it plays, queue rules,
  // timing), each in a floating window, and the room's endpoints (this
  // computer's speakers and nodes), which are dragged from room to room or
  // clicked to open their settings.
  import { onMount } from 'svelte'
  import { api } from './api'
  import FloatingWindow from './FloatingWindow.svelte'
  import NodeScreenSettings from './NodeScreenSettings.svelte'
  import NodeSettings from './NodeSettings.svelte'
  import { online, screenName } from './nodeInfo'
  import { app, toast } from './state.svelte'
  import type { HouseNode, OutputConfig, Room, RoomTiming, TimingSpeaker } from './types'
  import type { Visuals } from './visuals'

  type Window =
    | { kind: 'room' | 'display' | 'plays' | 'rules' | 'timing'; room: number }
    | { kind: 'node' | 'output'; id: number }

  let rooms = $state<Room[]>([])
  let outputs = $state<OutputConfig[]>([])
  let nodes = $state<HouseNode[]>([])
  let newName = $state('')
  let win = $state<Window | null>(null)
  /** Each room's "when quiet" choices. */
  let quiet = $state<Record<number, { follow_main: boolean; takeover: 'now' | 'after_song' }>>({})
  let rules = $state({ max_queue: 0, per_user_limit: 3, skip_votes: 0 })
  let timing = $state<RoomTiming | null>(null)
  let renameTo = $state('')
  let deleting = $state(false)
  /** The house's visualizers, where a node's own start from. */
  let house = $state<Visuals | null>(null)
  /** What's being dragged, and the room it's over. */
  let dragging = $state<{ kind: 'node' | 'output'; id: number } | null>(null)
  let over = $state<number | null>(null)

  async function load() {
    try {
      const [list, out, nodeList] = await Promise.all([api.rooms(), api.outputs(), api.nodes()])
      rooms = list.rooms
      app.rooms = list.rooms
      outputs = out.outputs
      nodes = nodeList.nodes.filter((n) => n.status === 'approved')
      const others = rooms.filter((r) => r.id !== 1)
      const answers = await Promise.all(others.map((r) => api.roomQuiet(r.id)))
      quiet = Object.fromEntries(others.map((r, i) => [r.id, answers[i]]))
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load rooms', true)
    }
  }

  onMount(() => {
    void load()
    // Nodes report every few seconds (their AI progress, whether they're up).
    const timer = setInterval(async () => {
      try {
        nodes = (await api.nodes()).nodes.filter((n) => n.status === 'approved')
      } catch {
        // The next one will do.
      }
    }, 5000)
    return () => clearInterval(timer)
  })

  async function run(action: () => Promise<unknown>, done: string) {
    try {
      await action()
      toast(done)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  function add(e: SubmitEvent) {
    e.preventDefault()
    const name = newName.trim()
    if (!name) return
    void run(() => api.createRoom(name), `Made ${name}`).then(() => (newName = ''))
  }

  const nameOf = (id: number) => rooms.find((r) => r.id === id)?.name ?? 'Main'
  const leaders = (room: Room) => rooms.filter((r) => r.id !== room.id && r.linked_to === null)
  const followers = (room: Room) => rooms.filter((r) => r.linked_to === room.id)
  const roomOf = (id: number) => rooms.find((r) => r.id === id)
  const playsText = (room: Room) => (room.linked_to ? `Along with ${nameOf(room.linked_to)}` : 'Its own music')

  /** Opens a window, loading what it shows first. */
  async function open(next: Window) {
    try {
      if (next.kind === 'rules') rules = await api.roomRules(next.room)
      if (next.kind === 'timing') {
        timing = null
        void api.roomTiming(next.room).then((t) => (timing = t))
      }
      if (next.kind === 'display' && !house) house = await api.visuals().catch(() => null)
      if (next.kind === 'room') {
        renameTo = roomOf(next.room)?.name ?? ''
        deleting = false
      }
      win = next
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not open it', true)
    }
  }

  async function nudge(room: number, speaker: TimingSpeaker, ms: number) {
    try {
      timing = await api.nudgeTiming(room, speaker.kind, speaker.id, ms)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change it', true)
    }
  }

  async function setScreen(node: HouseNode, screen: string, visual: Visuals | null) {
    try {
      await api.setNodeAudio(node.id, { ...node.audio, screen, visual })
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
    await load()
  }

  /** Puts a dragged endpoint in `room`. */
  function dropOn(room: Room) {
    const item = dragging
    dragging = null
    over = null
    if (!item) return
    if (item.kind === 'node') {
      const node = nodes.find((n) => n.id === item.id)
      if (node && node.room_id !== room.id) void run(() => api.setNodeRoom(node.id, room.id), `${node.name} moved to ${room.name}`)
    } else {
      const output = outputs.find((o) => o.id === item.id)
      if (output && output.room_id !== room.id)
        void run(() => api.placeOutput(output.id, room.id), `${output.label} moved to ${room.name}`)
    }
  }

  const winRoom = $derived(win && 'room' in win ? roomOf(win.room) : undefined)
  const winNode = $derived.by(() => {
    const w = win
    return w?.kind === 'node' ? nodes.find((n) => n.id === w.id) : undefined
  })
  const winOutput = $derived.by(() => {
    const w = win
    return w?.kind === 'output' ? outputs.find((o) => o.id === w.id) : undefined
  })
</script>

<section class="card wide">
  <h2>Rooms</h2>
  <p class="small muted">
    Each room is its own party: its own queue, playlist, volume and skip votes, played on the speakers and nodes you put
    in it. A room uses Main's queue rules (Admin &gt; Rules) until you give it its own. Drag an endpoint from one room to
    another to move it; click one to open its settings. People pick their room in the app, or scan a room's join code
    to walk straight in. Your song likes and what the party learns are shared across the house.
  </p>
  <form class="add" onsubmit={add}>
    <input bind:value={newName} placeholder="New room, like Patio" maxlength="40" aria-label="New room's name" />
    <button class="primary" disabled={!newName.trim()}>Add room</button>
  </form>
</section>

<div class="grid wide">
  {#each rooms as room (room.id)}
    <section class="card room">
      <div class="head">
        <h3>{room.name}</h3>
        <button class="small-button" onclick={() => open({ kind: 'room', room: room.id })}>Room options</button>
      </div>

      <div class="tools">
        <button onclick={() => open({ kind: 'display', room: room.id })}>Display settings</button>
        <button title="What this room plays" onclick={() => open({ kind: 'plays', room: room.id })}>
          Plays<span class="muted small"> · {playsText(room)}</span>
        </button>
        <button onclick={() => open({ kind: 'rules', room: room.id })}>Queue rules</button>
        {#if room.linked_to === null}
          <button onclick={() => open({ kind: 'timing', room: room.id })}>Timing</button>
        {:else}
          <button disabled title="Timing is set in the room this one plays along with">Timing</button>
        {/if}
      </div>

      <div
        class="endpoints"
        class:over={over === room.id}
        role="group"
        aria-label="Endpoints in {room.name}"
        ondragover={(e) => {
          if (!dragging) return
          e.preventDefault()
          over = room.id
        }}
        ondragleave={() => over === room.id && (over = null)}
        ondrop={(e) => {
          e.preventDefault()
          dropOn(room)
        }}
      >
        <h4>Endpoints</h4>
        <p class="small muted">Speakers and nodes. Drag one to another room, or click it for its settings.</p>
        <div class="chips">
          {#each outputs.filter((o) => o.room_id === room.id) as output (output.id)}
            <button
              class="chip"
              draggable="true"
              ondragstart={(e) => {
                dragging = { kind: 'output', id: output.id }
                e.dataTransfer?.setData('text/plain', `output:${output.id}`)
              }}
              ondragend={() => ((dragging = null), (over = null))}
              onclick={() => open({ kind: 'output', id: output.id })}
            >
              {output.label}<span class="muted small"> · this computer</span>
            </button>
          {/each}
          {#each nodes.filter((n) => n.room_id === room.id) as node (node.id)}
            <button
              class="chip"
              draggable="true"
              ondragstart={(e) => {
                dragging = { kind: 'node', id: node.id }
                e.dataTransfer?.setData('text/plain', `node:${node.id}`)
              }}
              ondragend={() => ((dragging = null), (over = null))}
              onclick={() => open({ kind: 'node', id: node.id })}
            >
              <span class="dot" class:on={online(node)}></span>
              {node.name}<span class="muted small"> · node</span>
            </button>
          {/each}
          {#if !outputs.some((o) => o.room_id === room.id) && !nodes.some((n) => n.room_id === room.id)}
            <span class="small muted">None yet. Drag one here.</span>
          {/if}
        </div>
      </div>
    </section>
  {/each}
</div>

{#if win && (win.kind === 'plays') && winRoom}
  {@const room = winRoom}
  <FloatingWindow title="{room.name}: what it plays" onclose={() => (win = null)}>
    <label class="field">
      Plays
      <select
        value={room.linked_to ?? 0}
        onchange={(e) => {
          const to = Number(e.currentTarget.value) || null
          void run(() => api.linkRoom(room.id, to), to ? `${room.name} plays along with ${nameOf(to)}` : `${room.name} plays its own music`)
        }}
      >
        <option value={0}>Its own music</option>
        {#each leaders(room) as other (other.id)}
          <option value={other.id}>Along with {other.name}, in step</option>
        {/each}
      </select>
    </label>
    {#if room.id !== 1 && room.linked_to === null && quiet[room.id]}
      <label class="switch">
        <input
          type="checkbox"
          checked={quiet[room.id].follow_main}
          onchange={(e) => {
            const next = { ...quiet[room.id], follow_main: e.currentTarget.checked }
            void run(() => api.setRoomQuiet(room.id, next), next.follow_main ? `${room.name} plays Main's music when quiet` : 'Saved')
          }}
        />
        When nothing's queued here, play along with Main
      </label>
      {#if quiet[room.id].follow_main}
        <label class="field">
          A request here takes over
          <select
            value={quiet[room.id].takeover}
            onchange={(e) => {
              const next = { ...quiet[room.id], takeover: e.currentTarget.value as 'now' | 'after_song' }
              void run(() => api.setRoomQuiet(room.id, next), 'Saved')
            }}
          >
            <option value="now">Right away</option>
            <option value="after_song">When Main's song ends</option>
          </select>
        </label>
      {/if}
    {/if}
    {#if followers(room).length}
      <p class="small muted">Playing along here: {followers(room).map((r) => r.name).join(', ')}.</p>
    {/if}
    <p class="small muted">These save as you change them.</p>
  </FloatingWindow>
{:else if win && win.kind === 'rules' && winRoom}
  {@const room = winRoom}
  <FloatingWindow title="{room.name}: queue rules" subtitle="A room uses Main's rules until it has its own." onclose={() => (win = null)}>
    <form
      id="rules-form"
      class="stack"
      onsubmit={(e) => {
        e.preventDefault()
        void run(() => api.setRoomRules(room.id, rules), `${room.name}'s rules saved`).then(() => (win = null))
      }}
    >
      <label class="field">Most songs in the queue (0 for no limit) <input type="number" min="0" max="1000" bind:value={rules.max_queue} /></label>
      <label class="field">Songs each person can have queued <input type="number" min="0" max="100" bind:value={rules.per_user_limit} /></label>
      <label class="field">Votes needed to skip a song (0 turns voting off) <input type="number" min="0" max="100" bind:value={rules.skip_votes} /></label>
    </form>
    {#snippet footer()}
      <button onclick={() => (win = null)}>Cancel</button>
      <button class="primary" form="rules-form">Save and apply</button>
    {/snippet}
  </FloatingWindow>
{:else if win && win.kind === 'timing' && winRoom}
  {@const room = winRoom}
  <FloatingWindow title="{room.name}: timing" onclose={() => (win = null)}>
    <p class="small muted">
      Play the timing sound, then move each speaker earlier or later until the tocks land together. Nothing can play
      before the music reaches it, so moving one earlier holds the others back instead. Nodes change within a few
      seconds; this computer's speakers at once.
    </p>
    <div>
      <button onclick={() => run(() => api.playSyncTone(room.id), `Timing sound playing in ${room.name} for 30 seconds`)}>
        Play the timing sound (30 s)
      </button>
    </div>
    {#if !timing}
      <p class="small muted">Loading...</p>
    {:else if !timing.speakers.length}
      <p class="small muted">No speakers or nodes in this room yet.</p>
    {:else}
      <ul class="timing">
        {#each timing.speakers as speaker (speaker.kind + speaker.id)}
          <li>
            <span class="who">
              {speaker.name}
              <span class="small muted">{speaker.kind === 'node' ? 'node' : 'this computer'}, held back {speaker.delay_ms} ms</span>
            </span>
            <span class="nudges">
              <button class="small-button" aria-label="{speaker.name} 10 ms earlier" onclick={() => nudge(room.id, speaker, -10)}>-10</button>
              <button class="small-button" aria-label="{speaker.name} 1 ms earlier" onclick={() => nudge(room.id, speaker, -1)}>-1</button>
              <button class="small-button" aria-label="{speaker.name} 1 ms later" onclick={() => nudge(room.id, speaker, 1)}>+1</button>
              <button class="small-button" aria-label="{speaker.name} 10 ms later" onclick={() => nudge(room.id, speaker, 10)}>+10</button>
            </span>
          </li>
        {/each}
      </ul>
      <p class="small muted">- is earlier, + is later, in milliseconds. Changes apply as you press them.</p>
    {/if}
  </FloatingWindow>
{:else if win && win.kind === 'display' && winRoom}
  {@const room = winRoom}
  <FloatingWindow title="{room.name}: display" subtitle="What the room's screens show." onclose={() => (win = null)}>
    {#each nodes.filter((n) => n.room_id === room.id) as node (node.id)}
      <section class="stack">
        <h3>{node.name} <span class="small muted">shows: {screenName(node.audio.screen)}</span></h3>
        <NodeScreenSettings
          screen={node.audio.screen ?? 'tv'}
          visual={node.audio.visual ?? null}
          start={house}
          onchange={(screen, visual) => setScreen(node, screen, visual)}
        />
      </section>
    {:else}
      <p class="small muted">No node with a screen in this room yet.</p>
    {/each}
    <section class="stack">
      <h3>Open this room's pages</h3>
      <div class="links">
        <a href="/tv?room={room.id}" target="_blank" rel="noopener">TV page</a>
        <a href="/lyrics?room={room.id}" target="_blank" rel="noopener">Lyrics page</a>
        <a href="/api/join/qr.svg?room={room.id}" target="_blank" rel="noopener">Join code</a>
      </div>
    </section>
    <p class="small muted">These save as you change them.</p>
  </FloatingWindow>
{:else if win && win.kind === 'room' && winRoom}
  {@const room = winRoom}
  <FloatingWindow title="{room.name}: options" onclose={() => (win = null)}>
    <form
      class="add"
      onsubmit={(e) => {
        e.preventDefault()
        void run(() => api.renameRoom(room.id, renameTo), 'Renamed')
      }}
    >
      <input bind:value={renameTo} maxlength="40" aria-label="Room name" />
      <button class="primary" disabled={!renameTo.trim() || renameTo.trim() === room.name}>Rename</button>
    </form>
    {#if room.id !== 1}
      <section class="stack">
        {#if deleting}
          <p class="small muted">Its speakers, nodes, people and queued songs move to Main.</p>
          <div class="add">
            <button class="danger" onclick={() => run(() => api.deleteRoom(room.id), `Deleted ${room.name}`).then(() => (win = null))}>Delete {room.name}</button>
            <button onclick={() => (deleting = false)}>Cancel</button>
          </div>
        {:else}
          <div><button onclick={() => (deleting = true)}>Delete this room</button></div>
        {/if}
      </section>
    {/if}
  </FloatingWindow>
{:else if win && win.kind === 'node' && winNode}
  <FloatingWindow title={winNode.name} subtitle="Node in {nameOf(winNode.room_id)}" onclose={() => (win = null)}>
    <NodeSettings node={winNode} {rooms} onchange={load} onclose={() => (win = null)} />
  </FloatingWindow>
{:else if win && win.kind === 'output' && winOutput}
  {@const output = winOutput}
  <FloatingWindow title={output.label} subtitle="This computer's speaker" onclose={() => (win = null)}>
    <label class="field">
      Room
      <select
        value={output.room_id}
        onchange={(e) => run(() => api.placeOutput(output.id, Number(e.currentTarget.value)), `${output.label} moved`)}
      >
        {#each rooms as room (room.id)}<option value={room.id}>{room.name}</option>{/each}
      </select>
    </label>
    <p class="small muted">
      Volume, which devices play and the rest of this computer's audio are in <a href="#/admin/audio">Audio</a>.
    </p>
  </FloatingWindow>
{/if}

<style>
  .add {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .add input {
    flex: 1;
    min-width: 160px;
  }

  /* The page lays its cards in columns; the rooms get a grid of their own. */
  .wide,
  .grid {
    grid-column: 1 / -1;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
    gap: 16px;
  }

  .room {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin: 0;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .head h3 {
    margin: 0;
  }

  .tools {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .tools button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0;
    text-align: left;
  }

  .endpoints {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
    border: 1px solid var(--edge, var(--line));
    border-radius: 14px;
    min-height: 110px;
  }

  .endpoints.over {
    border-color: var(--accent);
    box-shadow: 0 0 12px var(--glow);
  }

  .endpoints h4,
  .endpoints p {
    margin: 0;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 4px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: grab;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--line);
  }

  .dot.on {
    background: var(--ok);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 420px;
  }

  .field input[type='number'] {
    max-width: 140px;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .stack h3 {
    margin: 0;
    font-size: 1rem;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
  }

  .timing {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .timing li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .who {
    display: flex;
    flex-direction: column;
  }

  .nudges {
    display: flex;
    gap: 4px;
  }
</style>
