<script lang="ts">
  // One background, filling its box: the whole window behind a page, or a
  // small preview in the Admin picker.
  import { backgrounds } from './appearance.svelte'

  let {
    name,
    strength,
    fixed = false,
  }: { name: string; strength: number; fixed?: boolean } = $props()

  const image = $derived(
    name.startsWith('custom:')
      ? `/api/appearance/image?v=${name.slice(7)}`
      : (backgrounds.find((b) => b.id === name)?.image ?? ''),
  )
</script>

{#if name !== 'plain'}
  <div
    class="art {name}"
    class:fixed
    style:--strength={strength / 100}
    style:background-image={image ? `url('${image}')` : null}
    aria-hidden="true"
  ></div>
{/if}

<style>
  .art {
    position: absolute;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    background-position: center;
    background-size: cover;
    background-repeat: no-repeat;
    opacity: var(--strength);
    pointer-events: none;
  }

  /* The tallest the window gets, so it doesn't resize (and repaint) as a phone's address bar comes and goes. */
  .art.fixed {
    position: fixed;
    inset: auto;
    top: 0;
    left: 0;
    width: 100%;
    height: 100lvh;
    z-index: -1;
    will-change: transform;
  }

  /* Soft northern lights drawn in colors alone. */
  .aurora {
    background:
      radial-gradient(60% 50% at 20% 15%, rgb(63 224 207 / 0.75), transparent 70%),
      radial-gradient(55% 55% at 80% 25%, rgb(180 140 255 / 0.85), transparent 70%),
      radial-gradient(70% 50% at 55% 95%, rgb(124 64 200 / 0.8), transparent 70%);
  }

  /* A neon floor: lines running to the horizon. */
  .grid {
    background:
      linear-gradient(to bottom, transparent 55%, rgb(180 140 255 / 0.5) 100%),
      repeating-linear-gradient(to right, rgb(180 140 255 / 0.55) 0 1px, transparent 1px 64px),
      repeating-linear-gradient(to bottom, rgb(180 140 255 / 0.55) 0 1px, transparent 1px 64px);
    mask-image: linear-gradient(to bottom, transparent 10%, black 90%);
  }
</style>
