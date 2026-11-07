<script lang="ts">
  // Which room you're in: its queue, its music. Only shown when the house
  // has more than one room.
  import { app, switchRoom, toast } from './state.svelte'

  async function choose(e: Event) {
    const id = Number((e.currentTarget as HTMLSelectElement).value)
    const name = app.rooms.find((r) => r.id === id)?.name ?? 'that room'
    try {
      await switchRoom(id)
      toast(`You're in ${name} now`)
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not switch rooms', true)
    }
  }
</script>

{#if app.rooms.length > 1}
  <div class="room">
    <span class="muted small">Room</span>
    <select value={app.room} onchange={choose} aria-label="Your room">
      {#each app.rooms as room (room.id)}
        <option value={room.id}>
          {room.name}{room.linked_to ? ` (with ${app.rooms.find((r) => r.id === room.linked_to)?.name ?? 'another room'})` : ''}
        </option>
      {/each}
    </select>
  </div>
{/if}

<style>
  .room {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .room select {
    min-height: 34px;
    padding: 0 8px;
    font-size: 0.875rem;
  }
</style>
