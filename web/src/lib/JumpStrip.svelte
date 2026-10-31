<script lang="ts">
  // The A to Z strip on the side of a long list (or years, oldest to newest
  // or the other way round, following the list). Tap a letter or slide a
  // finger along it to jump. Letters and years missing from the list jump
  // to the next one there is, so the strip always looks the same.
  import type { Jump } from './types'

  let {
    jumps,
    years = false,
    newestFirst = false,
    below,
    inside = $bindable(false),
    onjump,
  }: {
    jumps: Jump[]
    years?: boolean
    /** The list runs from the newest year down. */
    newestFirst?: boolean
    /** The strip starts no higher than the top of this (the list), so it
     * doesn't cover what's above the list until that scrolls away. */
    below?: HTMLElement
    /** No room beside the list, so the strip is over its right edge: the
     * list should keep clear of it. */
    inside?: boolean
    onjump: (offset: number) => void
  } = $props()

  /** Below the search box stuck to the top of the page. */
  const TOP_PX = 84
  /** Room each label needs, in pixels. */
  const LABEL_PX = 16
  const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('')

  let height = $state(0)
  let top = $state(TOP_PX)
  /** Where the strip's left edge goes, just right of the list; null for
   * the screen's edge when there's no room. */
  let left = $state<number | null>(null)
  /** Gap between the list and the strip beside it. */
  const GAP_PX = 6
  let strip = $state<HTMLElement>()
  let pressed = $state<number | null>(null)
  let timer: ReturnType<typeof setTimeout> | undefined

  /** Every entry on the strip, top to bottom, with where it jumps. */
  const entries = $derived.by(() => {
    const at = new Map(jumps.map((j) => [j.label, j.offset]))
    let labels: string[]
    if (years) {
      const known = jumps.map((j) => Number(j.label)).filter((y) => Number.isFinite(y))
      if (!known.length) return []
      const [first, last] = [Math.min(...known), Math.max(...known)]
      labels = Array.from({ length: last - first + 1 }, (_, i) => String(first + i))
      if (newestFirst) labels.reverse()
    } else {
      labels = [...(at.has('#') ? ['#'] : []), ...LETTERS]
    }
    // A label with nothing under it jumps to the next one that has.
    const found: { label: string; offset: number }[] = []
    let next = jumps.length ? jumps[jumps.length - 1].offset : 0
    for (let i = labels.length - 1; i >= 0; i--) {
      next = at.get(labels[i]) ?? next
      found.unshift({ label: labels[i], offset: next })
    }
    return found
  })

  /** Which entries get a label when not all of them fit: evenly spread
   * letters, or round years (plus the first and last). */
  const shown = $derived.by(() => {
    const n = entries.length
    const room = Math.max(2, Math.floor(height / LABEL_PX))
    if (n <= room) return entries.map((_, i) => i)
    if (!years) {
      const step = Math.ceil(n / room)
      return entries.map((_, i) => i).filter((i) => i % step === 0)
    }
    const step = [2, 5, 10, 20, 25, 50, 100].find((s) => n / s + 2 <= room) ?? 100
    const gap = n / room
    const picked = [0]
    for (let i = 1; i < n - 1; i++) {
      if (Number(entries[i].label) % step === 0 && i - picked[picked.length - 1] >= gap && n - 1 - i >= gap) {
        picked.push(i)
      }
    }
    picked.push(n - 1)
    return picked
  })

  $effect(() => {
    if (!below) return
    const el = below
    let frame = 0
    const follow = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        const box = el.getBoundingClientRect()
        top = Math.max(TOP_PX, box.top)
        const width = years ? 36 : 24
        // Beside the list when the screen has room for it, whatever the
        // page's width.
        const beside = innerWidth - box.right >= width + GAP_PX
        left = beside ? box.right + GAP_PX : null
        inside = !beside
      })
    }
    follow()
    addEventListener('scroll', follow, { passive: true })
    addEventListener('resize', follow)
    return () => {
      cancelAnimationFrame(frame)
      removeEventListener('scroll', follow)
      removeEventListener('resize', follow)
    }
  })

  const place = (i: number) => `${((i + 0.5) / entries.length) * 100}%`

  function pick(y: number) {
    if (!strip || !entries.length) return
    const box = strip.getBoundingClientRect()
    const i = Math.max(0, Math.min(entries.length - 1, Math.floor(((y - box.top) / box.height) * entries.length)))
    if (i === pressed) return
    const first = pressed === null
    pressed = i
    // Jump straight away on touch, then at most every so often while
    // sliding, so the list isn't reloaded for every letter passed.
    clearTimeout(timer)
    if (first) onjump(entries[i].offset)
    else timer = setTimeout(() => pressed !== null && onjump(entries[pressed].offset), 90)
  }

  function down(e: PointerEvent) {
    strip?.setPointerCapture(e.pointerId)
    e.preventDefault()
    pick(e.clientY)
  }

  function up() {
    if (pressed !== null) {
      clearTimeout(timer)
      onjump(entries[pressed].offset)
    }
    pressed = null
  }
</script>

{#if entries.length > 1}
  <nav
    class="strip"
    class:years
    aria-label="Jump to"
    bind:this={strip}
    bind:clientHeight={height}
    style:top="{top}px"
    style:left={left === null ? null : `${left}px`}
    class:edge={left === null}
    onpointerdown={down}
    onpointermove={(e) => pressed !== null && pick(e.clientY)}
    onpointerup={up}
    onpointercancel={up}
  >
    {#each shown as i, n (i)}
      <button
        style:top={place(i)}
        aria-label="Jump to {entries[i].label}"
        onclick={() => onjump(entries[i].offset)}
        onpointerdown={(e) => e.preventDefault()}
      >
        {entries[i].label}
      </button>
      {#if n + 1 < shown.length && shown[n + 1] - i > 1}
        <span class="dot" style:top={place((i + shown[n + 1]) / 2)}></span>
      {/if}
    {/each}
    {#if pressed !== null}
      <div class="bubble" style:top={place(pressed)} aria-hidden="true">{entries[pressed].label}</div>
    {/if}
  </nav>
{/if}

<style>
  .strip {
    position: fixed;
    bottom: calc(76px + env(safe-area-inset-bottom));
    z-index: 6;
    width: 24px;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }

  /* No room beside the list (a phone): on the screen's edge. */
  .strip.edge {
    right: 0;
  }

  .strip.years {
    width: 36px;
  }

  button {
    position: absolute;
    left: 0;
    right: 0;
    min-height: 0;
    padding: 0;
    transform: translateY(-50%);
    border-radius: 6px;
    background: transparent;
    color: var(--accent);
    font-size: 0.6875rem;
    font-weight: 600;
    line-height: 1.2;
    font-variant-numeric: tabular-nums;
  }

  .years button {
    font-size: 0.625rem;
  }

  .dot {
    position: absolute;
    left: 50%;
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--muted);
    transform: translate(-50%, -50%);
  }

  .bubble {
    position: absolute;
    right: calc(100% + 12px);
    min-width: 56px;
    padding: 8px 12px;
    transform: translateY(-50%);
    border-radius: 14px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 1.5rem;
    font-weight: 700;
    text-align: center;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
</style>
