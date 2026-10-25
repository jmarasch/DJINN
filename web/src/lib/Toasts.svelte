<script lang="ts">
  import { dismissToast, toasts } from './state.svelte'
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <div class="toast" class:error={t.error}>
      {t.text}
      {#if t.action}
        <button
          onclick={() => {
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
