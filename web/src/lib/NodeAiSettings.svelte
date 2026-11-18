<script lang="ts">
  // A node's AI lyrics help: on or off, the model, and how it's going. In
  // Admin > Nodes and in a node's settings window on the Rooms page.
  import { t } from './i18n.svelte'
  import { api } from './api'
  import { aiPhase } from './nodeInfo'
  import { toast } from './state.svelte'
  import type { HouseNode } from './types'

  let { node, onchange }: { node: HouseNode; onchange: () => void | Promise<void> } = $props()

  const AI_MODELS = ['tiny', 'base', 'small', 'medium']

  function gb(mb: number | null): string {
    return mb ? `${Math.round(mb / 1024)} GB memory` : 'memory unknown'
  }

  async function setAi(help: boolean, model: string | null) {
    try {
      await api.setNodeAi(node.id, help, model)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
    await onchange()
  }
</script>

<div class="ai">
  <label class="switch">
    <input type="checkbox" checked={node.ai_help} onchange={(e) => setAi(e.currentTarget.checked, node.ai_model)} />
    <b>Help with AI lyrics</b>
  </label>
  <p class="small muted">
    Only while {node.name} plays nothing and its screen shows no visuals: it pauses the moment music or visuals start, so
    playback is never affected. It gets everything it needs from this server, never the internet, and deletes each song
    when done. Lyrics listening must be on in <a href="#/admin/ai">AI</a>.
  </p>
  <label>
    Model
    <select value={node.ai_model ?? ''} onchange={(e) => setAi(node.ai_help, e.currentTarget.value || null)}>
      <option value="">Suggested ({node.ai.suggested})</option>
      {#each AI_MODELS as m (m)}<option value={m}>{m}</option>{/each}
    </select>
  </label>
  <p class="small muted">
    {node.ai.threads ? `${node.ai.threads} threads` : 'Processor unknown'} · {gb(node.ai.memory_mb)}{node.ai.bench_ms
      ? ` · speed test: ${(node.ai.bench_ms / 1000).toFixed(1)} s to hear 30 s`
      : ''}{node.ai.secs_per_minute ? ` · about ${Math.round(node.ai.secs_per_minute)} s per minute of song` : ''}
  </p>
  {#if node.ai_help}
    <p class="small" class:bad={node.ai.phase === 'failed'}>
      {aiPhase(node.ai.phase) ?? t('ni.waiting_node')}{node.ai.detail ? `: ${node.ai.detail}` : ''}
    </p>
  {/if}
</div>

<style>
  .ai {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .ai p {
    margin: 0;
  }

  .bad {
    color: var(--bad);
  }
</style>
