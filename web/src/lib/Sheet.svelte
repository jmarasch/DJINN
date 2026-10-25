<script lang="ts">
  // A panel that slides up over the page. Closes on the backdrop, the
  // close button, or Escape.
  import type { Snippet } from 'svelte'
  import Icon from './Icon.svelte'

  let { title, onclose, children }: { title: string; onclose: () => void; children: Snippet } =
    $props()
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<div class="backdrop" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label={title}>
  <header>
    <h2>{title}</h2>
    <button class="close" aria-label="Close" onclick={onclose}><Icon name="close" /></button>
  </header>
  <div class="body">{@render children()}</div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    background: rgb(0 0 0 / 0.5);
  }

  .sheet {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 31;
    max-width: 560px;
    max-height: 88dvh;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    border-radius: 18px 18px 0 0;
    background: var(--surface);
    box-shadow: 0 -8px 40px rgb(0 0 0 / 0.35);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 12px 4px 20px;
  }

  h2 {
    margin: 0;
    color: var(--text);
    font-size: 1.125rem;
  }

  .close {
    width: 44px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .body {
    overflow-y: auto;
    padding: 4px 20px calc(20px + env(safe-area-inset-bottom));
  }
</style>
