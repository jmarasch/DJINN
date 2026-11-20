<script lang="ts">
  // A window that floats over the page for one group of settings: closes on
  // the X, Escape or a click outside it. `footer` holds its Save and apply
  // button; without one it gets a Done button (settings that save as you
  // change them).
  import { t } from './i18n.svelte'
  import type { Snippet } from 'svelte'
  import Icon from './Icon.svelte'

  let {
    title,
    subtitle = '',
    onclose,
    children,
    footer,
  }: { title: string; subtitle?: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props()
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<div class="backdrop" role="presentation" onclick={onclose}></div>
<div class="window" role="dialog" aria-modal="true" aria-label={title}>
  <header>
    <div class="titles">
      <h2>{title}</h2>
      {#if subtitle}<p class="small muted">{subtitle}</p>{/if}
    </div>
    <button class="close" aria-label={t('common.close')} onclick={onclose}><Icon name="close" /></button>
  </header>
  <div class="body">{@render children()}</div>
  <footer>
    {#if footer}
      {@render footer()}
    {:else}
      <button class="primary" onclick={onclose}>{t('common.done')}</button>
    {/if}
  </footer>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgb(0 0 0 / 0.55);
  }

  .window {
    position: fixed;
    z-index: 41;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(720px, calc(100vw - 24px));
    max-height: calc(100dvh - 32px);
    display: flex;
    flex-direction: column;
    border: 1px solid var(--edge-strong, var(--line));
    border-radius: 18px;
    background: var(--surface);
    box-shadow:
      0 0 24px var(--glow, transparent),
      0 12px 48px rgb(0 0 0 / 0.45);
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 12px 6px 20px;
  }

  .titles h2 {
    margin: 0;
    font-size: 1.2rem;
    color: var(--text);
  }

  .titles p {
    margin: 2px 0 0;
  }

  .close {
    width: 44px;
    flex: none;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .body {
    flex: 1;
    overflow-y: auto;
    padding: 6px 20px 14px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 20px 16px;
    border-top: 1px solid var(--line);
  }
</style>
