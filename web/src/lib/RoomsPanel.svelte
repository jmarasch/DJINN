<script lang="ts">
  // Admin > Rooms: areas with their own speakers, each its own party (queue,
  // rules, skip votes) unless linked to play along with another. Speakers on
  // this computer and nodes are placed in rooms here.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { HouseNode, OutputConfig, Room } from './types'

  let rooms = $state<Room[]>([])
  let outputs = $state<OutputConfig[]>([])
  let nodes = $state<HouseNode[]>([])
  let newName = $state('')
  let renaming = $state<number | null>(null)
  let renameTo = $state('')
  let deleting = $state<number | null>(null)

  async function load() {
    try {
      const [list, out, nodeList] = await Promise.all([api.rooms(), api.outputs(), api.nodes()])
      rooms = list.rooms
      app.rooms = list.rooms
      outputs = out.outputs
      nodes = nodeList.nodes.filter((n) => n.status === 'approved')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load rooms', true)
    }
  }

  onMount(() => void load())

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
</script>

<section class="card">
  <h2>Rooms</h2>
  <p class="small muted">
    Each room is its own party: its own queue, playlist, volume and skip votes, played on the speakers and nodes you
    put in it. Link a room to another to play the same music in step there. People pick their room in the app, or scan
    a room's join code to walk straight in. Your song likes and what the party learns are shared across the house.
  </p>
  <form class="add" onsubmit={add}>
    <input bind:value={newName} placeholder="New room, like Patio" maxlength="40" aria-label="New room's name" />
    <button class="primary" disabled={!newName.trim()}>Add room</button>
  </form>
</section>

{#each rooms as room (room.id)}
  <section class="card room">
    <div class="head">
      {#if renaming === room.id}
        <form
          class="add"
          onsubmit={(e) => {
            e.preventDefault()
            void run(() => api.renameRoom(room.id, renameTo), 'Renamed').then(() => (renaming = null))
          }}
        >
          <input bind:value={renameTo} maxlength="40" aria-label="Room name" />
          <button class="primary">Save</button>
          <button type="button" onclick={() => (renaming = null)}>Cancel</button>
        </form>
      {:else}
        <h3>{room.name}</h3>
        <button class="small-button" onclick={() => ((renaming = room.id), (renameTo = room.name))}>Rename</button>
        {#if room.id !== 1}
          {#if deleting === room.id}
            <button class="small-button danger" onclick={() => run(() => api.deleteRoom(room.id), `Deleted ${room.name}`)}
              >Delete {room.name}</button
            >
            <button class="small-button" onclick={() => (deleting = null)}>Cancel</button>
          {:else}
            <button class="small-button" onclick={() => (deleting = room.id)}>Delete</button>
          {/if}
        {/if}
      {/if}
    </div>
    {#if deleting === room.id}
      <p class="small muted">Its speakers, nodes, people and queued songs move to Main.</p>
    {/if}

    <label>
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
    {#if followers(room).length}
      <p class="small muted">Playing along here: {followers(room).map((r) => r.name).join(', ')}.</p>
    {/if}

    <div class="placed">
      <span class="small muted">Speakers and nodes:</span>
      {#each outputs.filter((o) => o.room_id === room.id) as output (output.id)}
        <span class="chip">{output.label}</span>
      {/each}
      {#each nodes.filter((n) => n.room_id === room.id) as node (node.id)}
        <span class="chip">{node.name} (node)</span>
      {/each}
      {#if !outputs.some((o) => o.room_id === room.id) && !nodes.some((n) => n.room_id === room.id)}
        <span class="small muted">none yet</span>
      {/if}
    </div>

    <div class="links">
      <a href="/api/join/qr.svg?room={room.id}" target="_blank" rel="noopener">Join code</a>
      <a href="/tv?room={room.id}" target="_blank" rel="noopener">TV page</a>
      <a href="/lyrics?room={room.id}" target="_blank" rel="noopener">Lyrics page</a>
    </div>
  </section>
{/each}

{#if rooms.length > 1}
  <section class="card">
    <h3>Where things play</h3>
    {#each outputs as output (output.id)}
      <div class="row-pick">
        <span>{output.label} <span class="muted small">(this computer)</span></span>
        <select
          aria-label="Room for {output.label}"
          value={output.room_id}
          onchange={(e) => run(() => api.placeOutput(output.id, Number(e.currentTarget.value)), `${output.label} moved`)}
        >
          {#each rooms as room (room.id)}<option value={room.id}>{room.name}</option>{/each}
        </select>
      </div>
    {/each}
    {#each nodes as node (node.id)}
      <div class="row-pick">
        <span>{node.name} <span class="muted small">(node)</span></span>
        <select
          aria-label="Room for {node.name}"
          value={node.room_id}
          onchange={(e) => run(() => api.setNodeRoom(node.id, Number(e.currentTarget.value)), `${node.name} moved`)}
        >
          {#each rooms as room (room.id)}<option value={room.id}>{room.name}</option>{/each}
        </select>
      </div>
    {/each}
  </section>
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

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .head h3 {
    flex: 1;
    margin: 0;
  }

  .room label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 10px;
  }

  .placed,
  .links {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
  }

  .row-pick {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }
</style>
