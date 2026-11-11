<script lang="ts">
  // A node's own screen once joined: Main's page for its room in a frame,
  // with the node's visualizer drawn underneath. The node measures what its
  // speaker plays (`/api/node/levels`) and draws it here; Main's page says
  // when it wants the visual (going see-through over it) and is told
  // whether there's sound to draw.
  import { onMount } from 'svelte'
  import { GlVisual } from './nodeGlVisual'
  import { draw, isGlStyle, Smoother, type ShapeStyle } from './nodeVisual'

  let { url }: { url: string } = $props()

  let frame = $state<HTMLIFrameElement | null>(null)
  let canvas = $state<HTMLCanvasElement | null>(null)
  let glCanvas = $state<HTMLCanvasElement | null>(null)
  /** A shader style is being drawn (on its own canvas, WebGL's). */
  let onChip = $state(false)
  /** What Main's page shows now; the visual is only drawn when asked for. */
  let mode = $state('')
  /** The latest levels, or null while the speaker isn't open. */
  let levels: number[] | null = null
  let heardAt = 0

  const origin = $derived(new URL(url).origin)
  const style = $derived(new URL(url).searchParams.get('visual') ?? 'bars')
  /** Levels arriving and a speaker open: the visual has something to draw. */
  const ready = () => levels !== null && performance.now() - heardAt < 2000

  function tell() {
    frame?.contentWindow?.postMessage({ djinn: 'visual-ready', ready: ready() }, origin)
  }

  onMount(() => {
    const events = new EventSource('/api/node/levels')
    events.onmessage = (e) => {
      levels = JSON.parse(e.data)
      heardAt = performance.now()
    }
    const onMessage = (e: MessageEvent) => {
      if (e.source !== frame?.contentWindow || e.origin !== origin) return
      if (e.data?.djinn === 'screen-mode' && typeof e.data.mode === 'string') {
        mode = e.data.mode
        tell()
      }
    }
    window.addEventListener('message', onMessage)
    const timer = setInterval(tell, 500)

    const sound = new Smoother()
    // A shader style, until this box shows it can't keep up (then Bars).
    let chip: GlVisual | null = null
    let shape: ShapeStyle = isGlStyle(style) ? 'bars' : (style as ShapeStyle)
    if (isGlStyle(style) && glCanvas) {
      try {
        chip = new GlVisual(glCanvas, style)
        onChip = true
      } catch (e) {
        console.warn('Visualizer falls back to bars:', e)
      }
    }
    let raf = 0
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop)
      if (mode !== 'visual') return
      sound.step(ready() ? levels : null, now)
      if (chip) {
        if (chip.draw(sound, now)) return
        console.warn(`The ${style} visualizer is too much for this box; showing bars`)
        chip = null
        onChip = false
        shape = 'bars'
      }
      const ctx = canvas?.getContext('2d')
      if (!canvas || !ctx) return
      const scale = window.devicePixelRatio || 1
      const w = Math.round(canvas.clientWidth * scale)
      const h = Math.round(canvas.clientHeight * scale)
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w
        canvas.height = h
      }
      draw(ctx, shape, sound)
    }
    raf = requestAnimationFrame(loop)
    return () => {
      events.close()
      window.removeEventListener('message', onMessage)
      clearInterval(timer)
      cancelAnimationFrame(raf)
    }
  })
</script>

<div class="stage">
  <canvas bind:this={canvas} class:shown={mode === 'visual' && !onChip}></canvas>
  <canvas bind:this={glCanvas} class:shown={mode === 'visual' && onChip}></canvas>
  <iframe bind:this={frame} src={url} title="DJINN screen" onload={tell}></iframe>
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
