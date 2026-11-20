<script lang="ts">
  // A node's own screen once joined (or Main's visualizer page, with Main's
  // own measurements): Main's page for its room in a frame,
  // with the node's visualizer drawn underneath. The node measures what its
  // speaker plays (`/api/node/levels`) and draws it here, taking turns
  // through the styles Main's settings give (`visuals`, the node's own or
  // the house's). Main's page says when it wants the visual (going
  // see-through over it) and which song plays, and is told whether there's
  // sound to draw.
  import { t } from './i18n.svelte'
  import { onMount } from 'svelte'
  import { GlVisual } from './nodeGlVisual'
  import { isGlStyle, ShapeVisual, Smoother, type ShapeStyle, type SoundFrame, type VisualStyle } from './nodeVisual'
  import { DEFAULT_VISUALS, lookOf, type Visuals } from './visuals'

  let {
    url,
    visuals,
    levels = '/api/node/levels',
  }: {
    url: string
    visuals: Visuals | null
    /** Where the measurements come from: this node's speaker, or Main's
     * own (the visualizer page). */
    levels?: string
  } = $props()

  let frame = $state<HTMLIFrameElement | null>(null)
  let canvas = $state<HTMLCanvasElement | null>(null)
  let glCanvas = $state<HTMLCanvasElement | null>(null)
  /** A shader style is being drawn (on its own canvas, WebGL's). */
  let onChip = $state(false)
  /** What Main's page shows now; the visual is only drawn when asked for. */
  let mode = $state('')
  /** The song Main's page says is playing. */
  let song = $state<number | null>(null)
  /** Which style in the rotation is showing. */
  let turn = $state(0)
  /** Styles this box showed it can't keep up with. */
  let tooMuch = $state<string[]>([])
  /** The latest measurement, or null while the speaker isn't open. */
  let latest: SoundFrame | null = null
  let heardAt = 0
  /** Main's page hasn't said a word yet: shown after a while, with the
   * address, since a blocked or unreachable frame is only black. */
  let unheard = $state(false)

  const settings = $derived(visuals ?? DEFAULT_VISUALS)
  const rotation = $derived(settings.styles.filter((s) => !tooMuch.includes(s)))
  const style = $derived<VisualStyle>(rotation.length ? rotation[turn % rotation.length] : 'bars')
  // Plain values, so a fresh copy of the same settings (the node's page
  // asks every few seconds) changes nothing.
  const intensity = $derived(lookOf(settings, style).intensity)
  const rainbow = $derived(lookOf(settings, style).colors === 'rainbow')
  const change = $derived(settings.change)
  const every = $derived(Math.max(5, settings.every_secs))

  const origin = $derived(new URL(url).origin)
  /** Levels arriving and a speaker open: the visual has something to draw. */
  const ready = () => latest !== null && performance.now() - heardAt < 2000

  function tell() {
    frame?.contentWindow?.postMessage({ djinn: 'visual-ready', ready: ready() }, origin)
  }

  // Taking turns: with each song, or on a timer.
  let lastSong: number | null = null
  $effect(() => {
    const now = song
    if (now === null || now === lastSong) return
    if (lastSong !== null && change === 'song') turn++
    lastSong = now
  })
  $effect(() => {
    if (change !== 'time') return
    const timer = setInterval(() => turn++, every * 1000)
    return () => clearInterval(timer)
  })
  $effect(() => {
    if (change === 'never') turn = 0
  })

  const sound = new Smoother()
  let chip: GlVisual | null = null
  let shape: ShapeVisual | null = null

  // A new style (or look): set up its drawer. The WebGL canvas keeps its
  // context; each shader drawer frees what it used when replaced.
  $effect(() => {
    const wanted = style
    const colorful = rainbow
    sound.gain = intensity
    chip?.dispose()
    chip = null
    shape = null
    onChip = false
    if (isGlStyle(wanted)) {
      if (!glCanvas) return
      try {
        chip = new GlVisual(glCanvas, wanted)
        chip.rainbow = colorful
        onChip = true
      } catch (e) {
        console.warn(`The ${wanted} visualizer won't run here:`, e)
        tooMuch = [...tooMuch, wanted]
      }
      return
    }
    const ctx = canvas?.getContext('2d')
    if (ctx) {
      shape = new ShapeVisual(ctx, wanted as ShapeStyle)
      shape.rainbow = colorful
    }
  })

  onMount(() => {
    const events = new EventSource(levels)
    events.onmessage = (e) => {
      latest = JSON.parse(e.data)
      heardAt = performance.now()
    }
    const onMessage = (e: MessageEvent) => {
      if (e.source !== frame?.contentWindow || e.origin !== origin) return
      if (e.data?.djinn === 'screen-mode' && typeof e.data.mode === 'string') {
        mode = e.data.mode
        unheard = false
        if (typeof e.data.song === 'number') song = e.data.song
        tell()
      }
    }
    window.addEventListener('message', onMessage)
    const timer = setInterval(tell, 500)
    const waited = setTimeout(() => {
      if (!mode) unheard = true
    }, 10_000)

    let raf = 0
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop)
      if (mode !== 'visual') return
      sound.step(ready() ? latest : null, now)
      if (chip) {
        if (chip.draw(sound, now)) return
        console.warn(`The ${style} visualizer is too much for this box; skipping it`)
        tooMuch = [...tooMuch, style]
        return
      }
      if (!canvas || !shape) return
      const scale = window.devicePixelRatio || 1
      const w = Math.round(canvas.clientWidth * scale)
      const h = Math.round(canvas.clientHeight * scale)
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w
        canvas.height = h
      }
      shape.draw(sound, now)
    }
    raf = requestAnimationFrame(loop)
    return () => {
      events.close()
      window.removeEventListener('message', onMessage)
      clearInterval(timer)
      clearTimeout(waited)
      cancelAnimationFrame(raf)
      chip?.dispose()
    }
  })
</script>

<div class="stage">
  <canvas bind:this={canvas} class:shown={mode === 'visual' && !onChip}></canvas>
  <canvas bind:this={glCanvas} class:shown={mode === 'visual' && onChip}></canvas>
  <iframe bind:this={frame} src={url} title={t('stage.title')} allow="fullscreen" onload={tell}></iframe>
  {#if unheard}
    <p class="unheard">
      Main's screen page isn't answering here. This box is asking for {url}. Open that address in this
      computer's browser to see why (a firewall, or the browser asking to allow local network access).
    </p>
  {/if}
</div>

<style>
  .stage {
    position: fixed;
    inset: 0;
    z-index: 10;
    background: #0d0b14;
  }

  canvas,
  iframe {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
  }

  .unheard {
    position: absolute;
    left: 1.5rem;
    right: 1.5rem;
    bottom: 1.5rem;
    margin: 0;
    padding: 0.8rem 1rem;
    border-radius: 12px;
    background: rgb(255 255 255 / 0.1);
    color: #d9d3ec;
    font-size: 1rem;
    pointer-events: none;
  }

  canvas {
    visibility: hidden;
  }

  canvas.shown {
    visibility: visible;
  }

  /* See-through where Main's page is; both pages share one color scheme,
     which the browser needs to keep a frame see-through. */
  iframe {
    background: transparent;
  }
</style>
