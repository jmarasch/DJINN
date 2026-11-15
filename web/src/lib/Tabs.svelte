<script lang="ts">
  // A row of tabs in one bar, the chosen one filled: Admin's sub-pages, the
  // nodes page and the library editor use it. `wide` spans every column on
  // the admin pages that lay their cards in columns.
  let {
    items,
    active,
    onselect,
    wide = false,
  }: { items: [string, string][]; active: string; onselect: (id: string) => void; wide?: boolean } = $props()
</script>

<div class="subtabs" class:wide role="tablist">
  {#each items as [id, label] (id)}
    <button role="tab" aria-selected={active === id} class:active={active === id} onclick={() => onselect(id)}>
      {label}
    </button>
  {/each}
</div>

<style>
  .subtabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0 0 14px;
    padding: 4px;
    border: 1px solid var(--edge);
    border-radius: 14px;
    background: var(--surface);
  }

  .subtabs button {
    flex: 1;
    min-height: 40px;
    padding: 0 12px;
    background: transparent;
    color: var(--muted);
  }

  .subtabs button.active {
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
    box-shadow: var(--glow-strong);
  }
</style>
