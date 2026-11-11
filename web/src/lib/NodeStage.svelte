<script lang="ts">
  // A node's own screen once joined: Main's page for its room in a frame,
  // with the node's visualizer drawn underneath. The node measures what its
  // speaker plays (`/api/node/levels`) and draws it here; Main's page says
  // when it wants the visual (going see-through over it) and is told
  // whether there's sound to draw.
  import { onMount } from 'svelte'
  import { draw, Smoother, type VisualStyle } from './nodeVisual'

  let { url }: { url: string } = $props()

  let frame = $state<HTMLIFrameElement | null>(null)
  let canvas = $state<HTMLCanvasElement | null>(null)
  /** What Main's page shows now; the visual is only drawn when asked for. */
  let mode = $state('')
  /** The latest levels, or null while the speaker isn't open. */
  let levels: number[] | null = null
  let heardAt = 0

  const origin = $derived(new URL(url).origin)
  const style = $derived((new URL(url).searchParams.get('visual') ?? 'bars') as VisualStyle)
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
    let raf = 0
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop)
      if (mode !== 'visual' || !canvas) return
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      const scale = window.devicePixelRatio || 1
      const w = Math.round(canvas.clientWidth * scale)
      const h = Math.round(canvas.clientHeight * scale)
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w
        canvas.height = h
      }
      sound.step(ready() ? levels : null, now)
      draw(ctx, style, sound)
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
  <canvas bind:this={canvas} class:shown={mode === 'visual'}></canvas>
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
