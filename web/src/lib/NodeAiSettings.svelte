<script lang="ts">
  // A node's AI lyrics help: on or off, the model, and how it's going. In
  // Admin > Nodes and in a node's settings window on the Rooms page.
  import { locale, t } from './i18n.svelte'
  import { api } from './api'
  import { aiPhase, aiWhy } from './nodeInfo'
  import { toast } from './state.svelte'
  import type { HouseNode } from './types'

  let { node, onchange }: { node: HouseNode; onchange: () => void | Promise<void> } = $props()

  const AI_MODELS = ['tiny', 'base', 'small', 'medium']

  function gb(mb: number | null): string {
    return mb ? t('nai.gb', { n: Math.round(mb / 1024) }) : t('nai.mem_unknown')
  }

  async function setAi(help: boolean, model: string | null) {
    try {
      await api.setNodeAi(node.id, help, model)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
    await onchange()
  }
</script>

<div class="ai">
  <label class="switch">
    <input type="checkbox" checked={node.ai_help} onchange={(e) => setAi(e.currentTarget.checked, node.ai_model)} />
    <b>{t('nai.help')}</b>
  </label>
  <p class="small muted">
    {t('nai.note_1', { name: node.name })} <a href="#/admin/ai">{t('admin.menu.ai')}</a>.
  </p>
  <label>
    {t('nai.model')}
    <select value={node.ai_model ?? ''} onchange={(e) => setAi(node.ai_help, e.currentTarget.value || null)}>
      <option value="">{t('nai.suggested', { model: node.ai.suggested })}</option>
      {#each AI_MODELS as m (m)}<option value={m}>{m}</option>{/each}
    </select>
  </label>
  <p class="small muted">
    {node.ai.threads ? t('nai.threads', { n: node.ai.threads }) : t('nai.cpu_unknown')} · {gb(node.ai.memory_mb)}{node.ai.bench_ms
      ? t('nai.bench', { secs: (node.ai.bench_ms / 1000).toLocaleString(locale.code, { maximumFractionDigits: 1, minimumFractionDigits: 1 }) })
      : ''}{node.ai.secs_per_minute ? t('nai.per_minute', { n: Math.round(node.ai.secs_per_minute) }) : ''}
  </p>
  {#if node.ai_help}
    <p class="small" class:bad={node.ai.phase === 'failed'}>
      {aiPhase(node.ai.phase) ?? t('ni.waiting_node')}{node.ai.detail ? `: ${aiWhy(node.ai.detail)}` : ''}
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
