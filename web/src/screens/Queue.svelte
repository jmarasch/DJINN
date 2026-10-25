<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import { app, isAdmin, toast } from '../lib/state.svelte'
  import type { QueueItem } from '../lib/types'

  const mine = $derived(app.queue.filter((q) => q.user_id === app.me?.id).length)

  async function remove(item: QueueItem) {
    const before = app.queue
    app.queue = app.queue.filter((q) => q.id !== item.id)
    try {
      await api.removeRequest(item.id)
    } catch (e) {
      app.queue = before
      toast(e instanceof Error ? e.message : 'Could not remove that song', true)
    }
  }

  async function move(item: QueueItem, index: number) {
    const before = app.queue
    const next = app.queue.filter((q) => q.id !== item.id)
    next.splice(index, 0, item)
    app.queue = next
    try {
      await api.moveRequest(item.id, index)
    } catch (e) {
      app.queue = before
      toast(e instanceof Error ? e.message : 'Could not move that song', true)
    }
  }
</script>

<h1>Queue</h1>
{#if app.queue.length === 0}
  <p class="muted">
    No requests yet. When the queue is empty, songs come from the auto pool.
  </p>
  <a class="button primary" href="#/search">Find a song</a>
{:else}
  <p class="muted small">
    {app.queue.length} {app.queue.length === 1 ? 'song' : 'songs'}{mine
      ? `, ${mine} from you (tap × to cancel yours)`
      : ''}
  </p>
  <ol>
    {#each app.queue as item, i (item.id)}
      <li>
        <Art hash={item.track.art_hash} size={44} />
        <div class="text">
          <div class="ellipsis">{item.track.title}</div>
          <div class="muted small ellipsis">
            {byline(item.track.artist)}
            {#if item.track.duration_ms}· {duration(item.track.duration_ms)}{/if}
          </div>
          <div class="who small">for {item.user_id === app.me?.id ? 'you' : item.username}</div>
        </div>
        {#if isAdmin()}
          <div class="order">
            <button aria-label="Move up" disabled={i === 0} onclick={() => move(item, i - 1)}>
              <Icon name="up" size={20} />
            </button>
            <button
              aria-label="Move down"
              disabled={i === app.queue.length - 1}
              onclick={() => move(item, i + 1)}
            >
              <Icon name="down" size={20} />
            </button>
          </div>
        {/if}
        {#if isAdmin() || item.user_id === app.me?.id}
          <button class="remove" aria-label="Remove {item.track.title}" onclick={() => remove(item)}>
            <Icon name="close" />
          </button>
        {/if}
      </li>
    {/each}
  </ol>
{/if}

<style>
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 68px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .who {
    color: var(--accent);
  }

  .order {
    display: flex;
    flex-direction: column;
  }

  .order button {
    width: 40px;
    height: 30px;
    color: var(--muted);
  }

  .remove {
    width: 44px;
    height: 44px;
    color: var(--muted);
  }
</style>
