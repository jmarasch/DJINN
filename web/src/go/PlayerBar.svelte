<script lang="ts">
  // Compact controls stay available on every page except Now Playing.
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import { route } from '../lib/router.svelte'
  import { current, next, player, previous, toggle } from './player.svelte'

  const song = $derived(current())
</script>

{#if song && route.path !== '/playing'}
  <div class="bar">
    <div class="progress" style:width="{player.duration ? (player.position / player.duration) * 100 : 0}%"></div>
    <a class="info" href="#/playing" aria-label="Open Now Playing">
      <Art hash={song.art_hash} size={44} />
      <span class="text">
        <span class="title">{song.title}</span>
        <span class="muted small">{song.artist ?? ''}</span>
      </span>
    </a>
    <button class="icon" aria-label="Previous" onclick={previous}><Icon name="previous" /></button>
    <button class="icon big" aria-label={player.playing ? 'Pause' : 'Play'} onclick={toggle}>
      <Icon name={player.playing ? 'pause' : 'play'} size={28} />
    </button>
    <button class="icon" aria-label="Next" onclick={next}><Icon name="skip" /></button>
  </div>
{/if}

<style>
  /* A neon bar above the tabs (or the page's bottom edge on wide screens). */
  .bar {
    position: fixed;
    left: 8px;
    right: 8px;
    bottom: calc(var(--tabs-height) + 8px + env(safe-area-inset-bottom));
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 12px;
    border: 1px solid var(--edge);
    border-radius: 16px;
    background: var(--card-bg);
    backdrop-filter: blur(14px);
    box-shadow: var(--glow);
  }

  @media (min-width: 900px) {
    .bar {
      left: calc(var(--side-width) + 16px);
      right: 16px;
      bottom: 16px;
    }
  }

  .progress {
    position: absolute;
    left: 14px;
    bottom: 0;
    height: 2px;
    border-radius: 2px;
    background: var(--accent);
    box-shadow: 0 0 8px var(--accent);
  }

  .info {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 0;
    color: inherit;
    text-align: left;
    text-decoration: none;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon {
    flex: none;
    width: 44px;
    min-height: 44px;
    padding: 0;
    background: transparent;
    color: var(--text);
  }
</style>
