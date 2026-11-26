<script module lang="ts">
  /** One button. With `reverse`, tapping it again while chosen switches to
   * that order, and an arrow shows which way it runs; `hint` and
   * `reverseHint` say so in words. */
  export interface SortOption<T extends string> {
    value: T
    label: string
    reverse?: T
    hint?: string
    reverseHint?: string
  }
</script>

<script lang="ts" generics="T extends string">
  import { t } from './i18n.svelte'
  // A small two-or-three way sort switch, remembered on this device.
  import { onMount } from 'svelte'

  let {
    options,
    value = $bindable(),
    storageKey,
    label = t('sort.label'),
  }: {
    options: SortOption<T>[]
    value: T
    storageKey: string
    label?: string
  } = $props()

  const known = (v: string) => options.some((o) => o.value === v || o.reverse === v)

  onMount(() => {
    try {
      const saved = localStorage.getItem(storageKey) as T | null
      if (saved && known(saved)) value = saved
    } catch {
      // Storage unavailable; keep the default.
    }
  })

  function pick(option: SortOption<T>) {
    const next = value === option.value && option.reverse ? option.reverse : option.value
    value = next
    try {
      localStorage.setItem(storageKey, next)
    } catch {
      // Not kept; fine.
    }
  }

  const reversed = (option: SortOption<T>) => !!option.reverse && value === option.reverse
  const chosen = (option: SortOption<T>) => value === option.value || reversed(option)
</script>

<div class="choice" role="group" aria-label={label}>
  {#each options as option (option.value)}
    <button
      class:active={chosen(option)}
      aria-pressed={chosen(option)}
      title={(reversed(option) ? option.reverseHint : option.hint) ?? undefined}
      onclick={() => pick(option)}
    >
      {option.label}{#if option.reverse && chosen(option)}<span class="arrow" aria-hidden="true">{reversed(option) ? '↑' : '↓'}</span
        ><span class="sr">{reversed(option) ? option.reverseHint : option.hint}</span>{/if}
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

  .arrow {
    margin-left: 4px;
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }
</style>
