<script lang="ts">
  // Library problems' Quarantine: every song of the problems ticked moved or
  // copied into another library folder, each keeping its place under its
  // own library folder. Shows what it would do (how many, how much space,
  // the room there) before anything happens; it's the editor's own move and
  // copy, so it can be undone from the editor's history.
  import { onMount } from 'svelte'
  import { api } from './api'
  import FloatingWindow from './FloatingWindow.svelte'
  import { locale, t } from './i18n.svelte'
  import { unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditableFolder, ProblemCount, ProblemKind, QuarantinePlan } from './types'

  let {
    problems,
    onclose,
    ondone,
  }: {
    /** The problems that list songs, with something in them. */
    problems: ProblemCount[]
    onclose: () => void
    ondone: () => void
  } = $props()

  /** The problems ticked; none at first. */
  let kinds = $state<ProblemKind[]>([])

  let folders = $state<EditableFolder[] | null>(null)
  let to = $state('')
  let copy = $state(false)
  let plan = $state<QuarantinePlan | null>(null)
  let looking = $state(false)
  let busy = $state(false)

  const n = (x: number) => x.toLocaleString(locale.code)
  const one = (x: number) => x.toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })
  const size = (bytes: number) =>
    bytes >= 1e12 ? `${one(bytes / 1e12)} TB` : bytes >= 1e9 ? `${one(bytes / 1e9)} GB` : `${n(Math.round(bytes / 1e6))} MB`

  const going = $derived(plan?.files.filter((f) => !f.problem) ?? [])
  const stuck = $derived(plan?.files.filter((f) => f.problem) ?? [])
  /** Why each file can't go, and how many. */
  const reasons = $derived(
    [...stuck.reduce((m, f) => m.set(f.problem ?? '', (m.get(f.problem ?? '') ?? 0) + 1), new Map<string, number>())].sort(
      (a, b) => b[1] - a[1],
    ),
  )
  const noRoom = $derived(plan !== null && plan.free !== null && plan.bytes > plan.free)

  onMount(async () => {
    try {
      folders = await api.editableFolders()
      to = folders.find((f) => f.editable)?.path ?? ''
    } catch (e) {
      toast(e instanceof Error ? e.message : t('qu.failed'), true)
    }
  })

  // Worked out again whenever the choice changes.
  $effect(() => {
    const [where, copying, ticked] = [to, copy, [...kinds]]
    if (!where || !ticked.length) {
      plan = null
      return
    }
    looking = true
    unlocked(() => api.quarantine(ticked, where, copying, true))
      .then((p) => {
        if (to === where && copy === copying && kinds.join() === ticked.join()) plan = p
      })
      .catch((e) => toast(e instanceof Error ? e.message : t('qu.failed'), true))
      .finally(() => (looking = false))
  })

  async function go() {
    busy = true
    try {
      const done = await unlocked(() => api.quarantine(kinds, to, copy, false))
      const count = done.files.filter((f) => !f.problem).length
      toast(t(copy ? 'qu.copied' : 'qu.moved', { count: n(count) }))
      ondone()
      onclose()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('qu.failed'), true)
    } finally {
      busy = false
    }
  }
</script>

<FloatingWindow title={t('qu.title')} {onclose}>
  <p class="small muted">{t('qu.intro')}</p>
  {#if folders === null}
    <p class="small muted">{t('common.loading')}</p>
  {:else if !folders.some((f) => f.editable)}
    <p class="small">{t('qu.none_writable')}</p>
  {:else}
    <fieldset>
      <legend>{t('qu.which')}</legend>
      {#each problems as p (p.kind)}
        <label class="choice">
          <input type="checkbox" value={p.kind} bind:group={kinds} />
          {t(`prob.${p.kind}`)} <span class="muted">({n(p.count)})</span>
        </label>
      {/each}
    </fieldset>
    <fieldset>
      <legend>{t('qu.how')}</legend>
      <label class="choice"><input type="radio" name="qu-how" value={false} bind:group={copy} /> {t('qu.move')}</label>
      <label class="choice"><input type="radio" name="qu-how" value={true} bind:group={copy} /> {t('qu.copy')}</label>
      <p class="small muted">{copy ? t('qu.copy_note') : t('qu.move_note')}</p>
    </fieldset>
    <label>
      {t('qu.to')}
      <select bind:value={to}>
        {#each folders as f (f.path)}
          <option value={f.path} disabled={!f.editable}>{f.path}{f.editable ? '' : ` (${t('qu.read_only')})`}</option>
        {/each}
      </select>
    </label>
    {#if plan}
      <ul class="facts small" class:dim={looking}>
        {#if going.length}
          <li class="muted">{t('qu.example')}<br /><span class="mono">{going[0].from}</span><br />→ <span class="mono">{going[0].to}</span></li>
        {/if}
        {#each reasons as [why, count] (why)}
          <li class="muted">{t('qu.skipped', { count: n(count), why })}</li>
        {/each}
      </ul>
    {/if}
  {/if}
  {#snippet footer()}
    {#if plan}
      <span class="needs small" class:dim={looking}>
        {t('qu.files', { count: n(going.length), size: size(plan.bytes) })}{#if plan.free !== null}<br /><span
            class:bad={noRoom}>{t('qu.free', { size: size(plan.free) })}{noRoom ? ` ${t('qu.no_room')}` : ''}</span
          >{/if}
      </span>
    {/if}
    <button onclick={onclose}>{t('common.cancel')}</button>
    <button class="primary" disabled={busy || looking || !going.length || (copy && noRoom)} onclick={go}>
      {busy ? t('qu.working') : t(copy ? 'qu.copy_n' : 'qu.move_n', { count: n(going.length) })}
    </button>
  {/snippet}
</FloatingWindow>

<style>
  fieldset {
    border: 0;
    margin: 0 0 12px;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .choice {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }

  .choice input {
    min-height: 0;
    margin: 0;
  }

  legend {
    margin-bottom: 4px;
  }

  .facts {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .needs {
    margin-right: auto;
  }

  .needs.dim,
  .facts.dim {
    opacity: 0.6;
  }

  .mono {
    font-family: ui-monospace, monospace;
    word-break: break-all;
  }

  .bad {
    color: var(--bad);
  }
</style>
