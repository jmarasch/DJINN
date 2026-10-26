<script lang="ts" generics="T extends string">
  // A small two-or-three way sort switch, remembered on this device.
  import { onMount } from 'svelte'

  let {
    options,
    value = $bindable(),
    storageKey,
    label = 'Sort',
  }: {
    options: { value: T; label: string }[]
    value: T
    storageKey: string
    label?: string
  } = $props()

  onMount(() => {
    try {
      const saved = localStorage.getItem(storageKey) as T | null
      if (saved && options.some((o) => o.value === saved)) value = saved
    } catch {
      // Storage unavailable; keep the default.
    }
  })

  function pick(next: T) {
    value = next
    try {
      localStorage.setItem(storageKey, next)
    } catch {
      // Not kept; fine.
    }
  }
</script>

<div class="choice" role="group" aria-label={label}>
  {#each options as option (option.value)}
    <button class:active={value === option.value} aria-pressed={value === option.value} onclick={() => pick(option.value)}>
      {option.label}
    </button>
  {/each}
</div>

<style>
  .choice {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: var(--surface);
  }

  button {
    min-height: 30px;
    padding: 0 12px;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: 0.8125rem;
  }

  button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }
</style>
