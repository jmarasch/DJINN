<script lang="ts">
  // The visualizer for this computer's own speakers (from the Playing tab,
  // or a screen plugged in here): the song, a small QR code and what's
  // coming up over a visual drawn in step with what the room's speakers
  // play. Public like the TV page; the styles and their turns are Admin >
  // Visualizers. Nothing playing shows the TV page instead.
  import { onMount } from 'svelte'
  import NodeStage from '../lib/NodeStage.svelte'
  import type { Visuals } from '../lib/visuals'

  const room = new URLSearchParams(location.search).get('room')
  const roomPart = room ? `room=${encodeURIComponent(room)}&` : ''
  const page = `${location.origin}/lyrics?${roomPart}order=visual,tv`
  const levels = `/api/tv/levels${room ? `?room=${encodeURIComponent(room)}` : ''}`

  let visuals = $state<Visuals | null>(null)
  let error = $state('')

  async function load() {
    try {
      const res = await fetch('/api/tv/visuals')
      if (!res.ok) {
        error = (await res.json().catch(() => null))?.error ?? 'The visualizer is not available.'
        return
      }
      visuals = await res.json()
      error = ''
    } catch {
      error = 'Cannot reach the jukebox. Retrying...'
      setTimeout(load, 5000)
    }
  }

  onMount(() => {
    document.title = 'Visualizer · DJINN'
    void load()
    // Changes in Admin > Visualizers reach a screen already showing it.
    const timer = setInterval(load, 30_000)
    return () => clearInterval(timer)
  })
</script>

{#if error}
  <p class="message">{error}</p>
{:else if visuals}
  <NodeStage url={page} {visuals} {levels} />
{/if}

<style>
  .message {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    background: #0d0b14;
    color: #fff;
    font-size: 1.5rem;
  }
</style>
