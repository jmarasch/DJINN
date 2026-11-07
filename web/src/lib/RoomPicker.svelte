<script lang="ts">
  import { t } from './i18n.svelte'
  // Which room you're in: its queue, its music. Only shown when the house
  // has more than one room.
  import { app, switchRoom, toast } from './state.svelte'

  async function choose(e: Event) {
    const id = Number((e.currentTarget as HTMLSelectElement).value)
    const name = app.rooms.find((r) => r.id === id)?.name ?? t('rooms.that_room')
    try {
      await switchRoom(id)
      toast(t('rooms.now_in', { room: name }))
    } catch (err) {
      toast(err instanceof Error ? err.message : t('rooms.switch_failed'), true)
    }
  }
</script>

{#if app.rooms.length > 1}
  <div class="room">
    <span class="muted small">{t('rooms.room')}</span>
    <select value={app.room} onchange={choose} aria-label={t('rooms.your_room')}>
      {#each app.rooms as room (room.id)}
        <option value={room.id}>
          {room.name}{room.linked_to ? ` (${t('rooms.with', { room: app.rooms.find((r) => r.id === room.linked_to)?.name ?? t('rooms.another_room') })})` : ''}
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
