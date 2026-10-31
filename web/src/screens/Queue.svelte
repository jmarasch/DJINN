<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import { app, askedBy, cancelRequest, involvement, isAdmin, requestTrack, toast } from '../lib/state.svelte'
  import type { QueueItem } from '../lib/types'

  const mine = $derived(app.queue.filter((q) => involvement(q) !== null).length)

  /** Songs with more backers play first, so a song only moves among those
   * with as many backers as it has. */
  const sameSupport = (a: QueueItem | undefined, b: QueueItem) => !!a && a.backers.length === b.backers.length

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
  <div class="empty">
    <img src="/images/empty-record.webp" alt="" />
    <p class="muted">No requests yet. When the queue is empty, songs come from the auto pool.</p>
    <a class="button primary" href="#/search">Find a song</a>
  </div>
{:else}
  <p class="muted small">
    {app.queue.length} {app.queue.length === 1 ? 'song' : 'songs'}{mine
      ? `, ${mine} from you (tap × to take yours back)`
      : ''}. Want a song that's already here? Tap <Icon name="up" size={14} /> to ask for it too: it moves up a
    place and uses one of your requests.
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
          <div class="who small">
            for {askedBy(item)}
            {#if item.backers.length}<span class="count">{item.backers.length + 1} asked</span>{/if}
          </div>
        </div>
        {#if involvement(item) === null && !app.pending.has(item.track.id)}
          <button class="metoo" aria-label="Ask for {item.track.title} too" onclick={() => requestTrack(item.track.id)}>
            <Icon name="up" />
          </button>
        {/if}
        {#if isAdmin()}
          <div class="order">
            <button
              aria-label="Move up"
              disabled={!sameSupport(app.queue[i - 1], item)}
              onclick={() => move(item, i - 1)}
            >
              <Icon name="up" size={20} />
            </button>
            <button
              aria-label="Move down"
              disabled={!sameSupport(app.queue[i + 1], item)}
              onclick={() => move(item, i + 1)}
            >
              <Icon name="down" size={20} />
            </button>
          </div>
        {/if}
        {#if isAdmin() || involvement(item) !== null}
          <button class="remove" aria-label="Remove {item.track.title}" onclick={() => cancelRequest(item.id)}>
            <Icon name="close" />
          </button>
        {/if}
      </li>
    {/each}
  </ol>
{/if}

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .empty img {
    width: min(190px, 55vw);
    height: auto;
  }

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

  .count {
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--surface-3);
    color: var(--text);
  }

  .metoo {
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
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
