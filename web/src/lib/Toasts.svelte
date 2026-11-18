<script lang="ts">
  import { dismissToast, toasts } from './state.svelte'
  import { t as tr } from './i18n.svelte'
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <!-- A click closes it; the keyboard has its action button, and it goes by itself. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="toast" class:error={t.error} onclick={() => dismissToast(t.id)} title={tr('toast.dismiss')}>
      {t.text}
      {#if t.action}
        <button
          onclick={(e) => {
            e.stopPropagation()
            t.action?.run()
            dismissToast(t.id)
          }}>{t.action.label}</button
        >
      {/if}
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    left: 16px;
    right: 16px;
    bottom: calc(76px + env(safe-area-inset-bottom));
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    pointer-events: none;
    z-index: 20;
  }

  .toast {
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 14px;
    max-width: 480px;
    pointer-events: auto;
    padding: 10px 16px;
    border-radius: 10px;
    background: var(--surface-3);
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.4);
  }

  .toast button {
    min-height: 32px;
    padding: 0 10px;
    background: transparent;
    color: var(--accent);
    font-weight: 600;
  }

  .toast.error {
    background: var(--bad-surface);
  }
</style>
