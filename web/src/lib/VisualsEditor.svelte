<script lang="ts">
  // Picks the visualizers a node screen takes turns with, and how each one
  // looks: a gallery of thumbnails, and a style's settings with a live
  // preview when one is opened. For the house (Admin > Visualizers) and for
  // a node of its own (its screen settings).
  import { onMount } from 'svelte'
  import type { VisualStyle } from './nodeVisual'
  import { DEFAULT_LOOK, lookOf, Preview, STYLES, styleInfo, thumbnail, type Look, type Visuals } from './visuals'

  let { value, onchange }: { value: Visuals; onchange: (visuals: Visuals) => void } = $props()

  let thumbs = $state<Record<string, string>>({})
  /** The style whose settings are open. */
  let open = $state<VisualStyle | null>(null)
  let previewCanvas = $state<HTMLCanvasElement | null>(null)

  onMount(() => {
    for (const { style } of STYLES) {
      void thumbnail(style).then((url) => (thumbs[style] = url))
    }
  })

  function save(change: Partial<Visuals>) {
    onchange({ ...value, ...change })
  }

  function toggle(style: VisualStyle, on: boolean) {
    const styles = on ? [...value.styles, style] : value.styles.filter((s) => s !== style)
    // At least one: the last can't be taken out.
    if (styles.length) save({ styles })
  }

  function move(style: VisualStyle, by: number) {
    const at = value.styles.indexOf(style)
    const to = at + by
    if (at < 0 || to < 0 || to >= value.styles.length) return
    const styles = [...value.styles]
    styles.splice(at, 1)
    styles.splice(to, 0, style)
    save({ styles })
  }

  function setLook(style: VisualStyle, change: Partial<Look>) {
    const look = { ...lookOf(value, style), ...change }
    const looks = { ...value.looks }
    if (look.intensity === DEFAULT_LOOK.intensity && look.colors === DEFAULT_LOOK.colors) delete looks[style]
    else looks[style] = look
    save({ looks })
  }

  // The open style, moving to the made-up song.
  const openLook = $derived(open ? lookOf(value, open) : DEFAULT_LOOK)
  const lookKey = $derived(`${openLook.intensity}|${openLook.colors}`)
  $effect(() => {
    const style = open
    const canvas = previewCanvas
    void lookKey
    if (!style || !canvas) return
    let preview: Preview
    try {
      preview = new Preview(canvas, style, openLook)
    } catch {
      return
    }
    const start = performance.now()
    let raf = 0
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame)
      preview.draw(now - start)
    }
    raf = requestAnimationFrame(frame)
    return () => {
      cancelAnimationFrame(raf)
      preview.close()
    }
  })
</script>

<div class="visuals">
  <div class="row">
    <label>
      Take turns
      <select value={value.change} onchange={(e) => save({ change: e.currentTarget.value as Visuals['change'] })}>
        <option value="song">With each song</option>
        <option value="time">Every few seconds</option>
        <option value="never">Never (only the first)</option>
      </select>
    </label>
    {#if value.change === 'time'}
      <label>
        Every {value.every_secs} seconds
        <input
          type="range"
          min="5"
          max="600"
          step="5"
          value={value.every_secs}
          onchange={(e) => save({ every_secs: Number(e.currentTarget.value) })}
        />
      </label>
    {/if}
  </div>
  <p class="small muted">
    Ticked ones take turns, in the numbered order. Open one for its settings and a preview with a made-up song.
  </p>

  <div class="gallery">
    {#each STYLES as info (info.style)}
      {@const at = value.styles.indexOf(info.style)}
      <div class="tile" class:on={at >= 0} class:open={open === info.style}>
        <button class="pick" onclick={() => (open = open === info.style ? null : info.style)}>
          {#if thumbs[info.style]}
            <img src={thumbs[info.style]} alt="" />
          {:else}
            <div class="blank"></div>
          {/if}
          <span class="name">{info.name}</span>
          {#if at >= 0}<span class="number">{at + 1}</span>{/if}
        </button>
        <label class="check small">
          <input type="checkbox" checked={at >= 0} onchange={(e) => toggle(info.style, e.currentTarget.checked)} />
          In the rotation
        </label>
      </div>
    {/each}
  </div>

  {#if open}
    {@const info = styleInfo(open)}
    {@const at = value.styles.indexOf(open)}
    <section class="detail">
      <!-- A fresh canvas each time: one can't go from 2D to WebGL drawing. -->
      {#key `${open}|${lookKey}`}<canvas bind:this={previewCanvas} width="640" height="360"></canvas>{/key}
      <div class="settings">
        <h3>{info?.name}</h3>
        <p class="small muted">{info?.about}</p>
        {#if info?.chip}
          <p class="small muted">
            Drawn on the graphics chip. A box that can't keep up skips it and moves on to the next.
          </p>
        {/if}
        <label class="check">
          <input type="checkbox" checked={at >= 0} onchange={(e) => toggle(open!, e.currentTarget.checked)} />
          In the rotation{at >= 0 ? `, number ${at + 1}` : ''}
        </label>
        {#if at >= 0 && value.styles.length > 1}
          <div class="moves">
            <button class="pill" disabled={at === 0} onclick={() => move(open!, -1)}>Earlier</button>
            <button class="pill" disabled={at === value.styles.length - 1} onclick={() => move(open!, 1)}>Later</button>
          </div>
        {/if}
        <label>
          How strongly it reacts: {Math.round(openLook.intensity * 100)}%
          <input
            type="range"
            min="0.25"
            max="3"
            step="0.05"
            value={openLook.intensity}
            onchange={(e) => setLook(open!, { intensity: Number(e.currentTarget.value) })}
          />
        </label>
        {#if open !== 'spectrogram' && open !== 'vu'}
          <label>
            Colors
            <select value={openLook.colors} onchange={(e) => setLook(open!, { colors: e.currentTarget.value as Look['colors'] })}>
              <option value="default">Its own</option>
              <option value="rainbow">Rainbow, turning slowly</option>
            </select>
          </label>
        {/if}
        <button onclick={() => (open = null)}>Close</button>
      </div>
    </section>
  {/if}
</div>

<style>
  .visuals {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
  }

  .gallery {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 12px;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .pick {
    position: relative;
    display: block;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 10px;
    overflow: hidden;
    background: #000;
    aspect-ratio: 16 / 9;
  }

  .tile.on .pick {
    border-color: var(--accent);
  }

  .tile.open .pick {
    outline: 2px solid var(--text);
    outline-offset: 2px;
  }

  .pick img,
  .blank {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .tile:not(.on) img {
    opacity: 0.55;
  }

  .name {
    position: absolute;
    left: 6px;
    bottom: 4px;
    color: #fff;
    font-size: 0.8125rem;
    font-weight: 600;
    text-shadow: 0 1px 3px #000;
  }

  .number {
    position: absolute;
    top: 4px;
    right: 4px;
    min-width: 22px;
    height: 22px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--on-accent, #fff);
    font-size: 0.75rem;
    font-weight: 700;
    line-height: 22px;
    text-align: center;
  }

  .detail {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
    gap: 16px;
    padding: 12px;
    border-radius: 12px;
    background: var(--surface-2);
  }

  .detail canvas {
    width: 100%;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    background: #000;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .settings h3 {
    margin: 0;
  }

  .settings p {
    margin: 0;
  }

  .moves {
    display: flex;
    gap: 6px;
  }

  @media (max-width: 640px) {
    .detail {
      grid-template-columns: 1fr;
    }
  }
</style>
