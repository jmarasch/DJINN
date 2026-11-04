<script lang="ts">
  // A sideways row that shows there's more: the cut-off side fades, and on
  // devices with a mouse there are arrows that move it about a page. Each
  // side only shows while there's more that way. Swiping and scrolling work
  // as in any scrolling row.
  import type { Snippet } from 'svelte'
  import Icon from './Icon.svelte'

  let { children, gap = 14 }: { children: Snippet; gap?: number } = $props()

  let row = $state<HTMLDivElement>()
  let more = $state({ left: false, right: false })

  function measure() {
    if (!row) return
    const { scrollLeft, scrollWidth, clientWidth } = row
    // Scroll positions can be fractions, so a pixel of slack counts as the end.
    more = { left: scrollLeft > 1, right: scrollLeft + clientWidth < scrollWidth - 1 }
  }

  function page(direction: -1 | 1) {
    if (!row) return
    const smooth = !window.matchMedia('(prefers-reduced-motion: reduce)').matches
    row.scrollBy({ left: direction * row.clientWidth * 0.8, behavior: smooth ? 'smooth' : 'auto' })
  }

  $effect(() => {
    const element = row
    if (!element) return
    measure()
    // The row changes size with the window, and what fits changes with its contents.
    const resized = new ResizeObserver(measure)
    resized.observe(element)
    const changed = new MutationObserver(measure)
    changed.observe(element, { childList: true })
    return () => {
      resized.disconnect()
      changed.disconnect()
    }
  })
</script>

<div class="scroll-row">
  <div
    class="row"
    class:fade-left={more.left}
    class:fade-right={more.right}
    style:gap="{gap}px"
    bind:this={row}
    onscroll={measure}
  >
    {@render children()}
  </div>
  {#if more.left}
    <button type="button" class="arrow left" aria-label="Scroll left" onclick={() => page(-1)}>
      <Icon name="chevron-left" />
    </button>
  {/if}
  {#if more.right}
    <button type="button" class="arrow right" aria-label="Scroll right" onclick={() => page(1)}>
      <Icon name="chevron-right" />
    </button>
  {/if}
</div>

<style>
  .scroll-row {
    position: relative;
    min-width: 0;
  }

  .row {
    --left: 0px;
    --right: 0px;
    display: flex;
    overflow-x: auto;
    padding-bottom: 6px;
    scrollbar-width: none;
    -webkit-mask-image: linear-gradient(
      to right,
      transparent,
      #000 var(--left),
      #000 calc(100% - var(--right)),
      transparent
    );
    mask-image: linear-gradient(
      to right,
      transparent,
      #000 var(--left),
      #000 calc(100% - var(--right)),
      transparent
    );
  }

  .row::-webkit-scrollbar {
    display: none;
  }

  .fade-left {
    --left: 40px;
  }

  .fade-right {
    --right: 40px;
  }

  .arrow {
    display: none;
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    width: 36px;
    height: 36px;
    padding: 0;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
  }

  .left {
    left: 2px;
  }

  .right {
    right: 2px;
  }

  /* Touch screens swipe, and the fade already shows there's more. */
  @media (any-hover: hover) {
    .arrow {
      display: flex;
    }
  }
</style>
