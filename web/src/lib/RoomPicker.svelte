<script lang="ts">
  import { t } from './i18n.svelte'
  // Which room you're in: its queue, its music. Only shown when the house
  // has more than one room. A neon button that opens the list of rooms.
  import Icon from './Icon.svelte'
  import { app, switchRoom, toast } from './state.svelte'

  /** Opens the list above the button, for the left bar where there's nothing below. */
  let { up = false, inline = false }: { up?: boolean; inline?: boolean } = $props()

  let open = $state(false)
  let box = $state<HTMLElement>()
  const current = $derived(app.rooms.find((r) => r.id === app.room))

  const label = (room: (typeof app.rooms)[number]) =>
    room.linked_to
      ? `${room.name} (${t('rooms.with', { room: app.rooms.find((r) => r.id === room.linked_to)?.name ?? t('rooms.another_room') })})`
      : room.name

  async function choose(id: number) {
    open = false
    if (id === app.room) return
    const name = app.rooms.find((r) => r.id === id)?.name ?? t('rooms.that_room')
    try {
      await switchRoom(id)
      toast(t('rooms.now_in', { room: name }))
    } catch (err) {
      toast(err instanceof Error ? err.message : t('rooms.switch_failed'), true)
    }
  }
</script>

<svelte:window
  onclick={(e) => {
    if (open && box && !box.contains(e.target as Node)) open = false
  }}
  onkeydown={(e) => {
    if (open && e.key === 'Escape') open = false
  }}
/>

{#if app.rooms.length > 1}
  <div class="room" bind:this={box}>
    <button
      class="room-button"
      aria-haspopup="listbox"
      aria-expanded={open}
      aria-label={t('rooms.your_room')}
      onclick={() => (open = !open)}
    >
      <Icon name="home" size={20} />
      <span class="room-text">
        <small>{t('rooms.room')}</small>
        <strong class="ellipsis">{current ? label(current) : ''}</strong>
      </span>
      <Icon name={open ? 'up' : 'down'} size={20} />
    </button>
    {#if open}
      <ul class="room-list" class:up class:inline role="listbox" aria-label={t('rooms.your_room')}>
        {#each app.rooms as room (room.id)}
          <li>
            <button
              role="option"
              aria-selected={room.id === app.room}
              class:current={room.id === app.room}
              onclick={() => choose(room.id)}
            >
              {label(room)}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .room {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .room-button {
    justify-content: flex-start;
    gap: 12px;
    width: 100%;
    padding: 8px 14px;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
    box-shadow: var(--glow);
    color: var(--accent);
    text-align: left;
  }

  .room-button:hover,
  .room-button[aria-expanded='true'] {
    border-color: var(--edge-strong);
    box-shadow: var(--glow-strong);
  }

  .room-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    line-height: 1.2;
  }

  .room-text small {
    color: var(--muted);
    font-size: 0.6875rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .room-text strong {
    color: var(--text);
    font-weight: 500;
  }

  .ellipsis {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* Floats over what's below, so opening it moves nothing. */
  .room-list {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    z-index: 30;
    min-width: 200px;
    margin: 0;
    padding: 4px;
    list-style: none;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--glow-strong);
  }

  /* In the phone menu the list just opens in place. */
  .room-list.inline {
    position: static;
    margin-top: 6px;
    box-shadow: none;
  }

  .room-list.up {
    top: auto;
    bottom: calc(100% + 6px);
  }

  .room-list button {
    justify-content: flex-start;
    width: 100%;
    min-height: 38px;
    padding: 0 12px;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    text-align: left;
  }

  .room-list button:hover {
    color: var(--text);
  }

  .room-list button.current {
    background: linear-gradient(90deg, rgb(180 140 255 / 0.4), rgb(180 140 255 / 0.18));
    color: var(--text);
  }
</style>
