<script lang="ts">
  // A warning on a screen page (TV, lyrics, visualizer, a node's screen)
  // when Windows' display scaling isn't 100%: scaled, the page is drawn at
  // the wrong size and the visualizers blur or misplace. The browser's own
  // zoom is told apart (the window's outer width is in screen points, the
  // page's inner width in page points), so only Windows' scale counts. It
  // shows for a while and goes, since a screen may have no mouse to close
  // it; it comes back when the page loads again or the scale changes.
  import { onMount } from 'svelte'

  const SHOW_FOR_MS = 60_000

  let scale = $state(1)
  let shown = $state(false)
  let timer: ReturnType<typeof setTimeout> | undefined

  /** Windows' scale, as a fraction (1.25 for 125%), with the browser's zoom taken out. */
  function windowsScale(): number {
    const zoom = window.outerWidth && window.innerWidth ? window.outerWidth / window.innerWidth : 1
    // A scrollbar or window border makes the two differ a little; a zoom
    // step is at least 10%.
    const browserZoom = Math.abs(zoom - 1) < 0.05 ? 1 : zoom
    return window.devicePixelRatio / browserZoom
  }

  function check() {
    const now = Math.round(windowsScale() * 100) / 100
    const changed = now !== scale
    scale = now
    if (Math.abs(now - 1) < 0.05) {
      shown = false
      return
    }
    if (changed || !timer) {
      shown = true
      clearTimeout(timer)
      timer = setTimeout(() => (shown = false), SHOW_FOR_MS)
    }
  }

  onMount(() => {
    // Only on Windows, and once per screen: not in a page framed by another.
    if (!/Windows/i.test(navigator.userAgent) || window.parent !== window) return
    scale = 1
    check()
    window.addEventListener('resize', check)
    return () => {
      window.removeEventListener('resize', check)
      clearTimeout(timer)
    }
  })
</script>

{#if shown}
  <div class="scale-warning" role="alert">
    <strong>Windows display scaling is set to {Math.round(scale * 100)}%</strong>
    <p>
      Scaling can make this screen look wrong. Set it to 100% in Windows Settings &gt; System &gt; Display &gt; Scale.
      If you want things on the display bigger, lower the screen resolution instead.
    </p>
    <button onclick={() => (shown = false)}>Got it</button>
  </div>
{/if}

<style>
  .scale-warning {
    position: fixed;
    z-index: 100;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    width: min(560px, calc(100vw - 32px));
    padding: 16px 18px;
    border: 2px solid #e8a317;
    border-radius: 14px;
    background: rgb(20 16 8 / 0.94);
    color: #fff;
    box-shadow: 0 0 24px rgb(232 163 23 / 0.45);
    font-size: 1rem;
    line-height: 1.4;
  }

  p {
    margin: 6px 0 12px;
  }

  button {
    min-height: 36px;
    padding: 0 14px;
    border: 0;
    border-radius: 8px;
    background: #e8a317;
    color: #000;
    font-weight: 700;
    cursor: pointer;
  }
</style>
