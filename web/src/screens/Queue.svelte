<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import PageTitle from '../lib/PageTitle.svelte'
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

<PageTitle title={t('queue.title')} subtitle={t('queue.subtitle')} />
{#if app.queue.length === 0}
  <div class="empty">
    <img src="/images/empty-queue.webp" alt="" />
    <p class="muted">{t('queue.empty')}</p>
    <a class="button primary" href="#/search">{t('common.find_a_song')}</a>
  </div>
{:else}
  <p class="muted small">
    {t('queue.songs', { n: app.queue.length })}{mine ? t('queue.yours', { n: mine }) : ''}.
    {t('queue.ask_too_before')} <Icon name="up" size={14} /> {t('queue.ask_too_after')}
  </p>
  <ol>
    {#each app.queue as item, i (item.id)}
      <li class="neon">
        <span class="num">{i + 1}</span>
        <Art hash={item.track.art_hash} size={56} round={10} />
        <div class="text">
          <div class="ellipsis">{item.track.title}</div>
          <div class="muted small ellipsis">
            {byline(item.track.artist)}
            {#if item.track.duration_ms}· {duration(item.track.duration_ms)}{/if}
          </div>
          <div class="who small">
            {t('queue.for', { name: askedBy(item) })}
          </div>
        </div>
        {#if item.backers.length}
          <span class="people" title={t('queue.asked', { n: item.backers.length + 1 })}>
            <Icon name="person" size={16} />
            {item.backers.length + 1}
          </span>
        {/if}
        {#if involvement(item) === null && !app.pending.has(item.track.id)}
          <button class="metoo" aria-label={t('queue.ask_for_too', { title: item.track.title })} onclick={() => requestTrack(item.track.id)}>
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
          <button class="remove" aria-label={t('queue.remove', { title: item.track.title })} onclick={() => cancelRequest(item.id)}>
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

  ol {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 76px;
    padding: 10px 12px;
    border-radius: 16px;
  }

  .num {
    flex: none;
    width: 22px;
    color: var(--text);
    font-size: 1.15rem;
    font-weight: 600;
    text-align: center;
  }

  .text .ellipsis:first-child {
    font-size: 1.05rem;
    font-weight: 600;
  }

  .people {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border: 1px solid var(--edge);
    border-radius: 10px;
    background: var(--surface-2);
    font-weight: 600;
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .who {
    color: var(--accent);
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
