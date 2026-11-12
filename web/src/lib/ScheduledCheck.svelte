<script lang="ts">
  // The nightly library check: on or off, its hour, and what the last one
  // found, to read in the morning.
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { CheckSummary, ScheduledCheck } from './types'

  let check = $state<ScheduledCheck | null>(null)

  $effect(() => {
    api
      .scheduledCheck()
      .then((c) => (check = c))
      .catch((e) => toast(e.message, true))
  })

  async function save() {
    if (!check) return
    try {
      check = await api.saveScheduledCheck(check.on, check.hour)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save that', true)
    }
  }

  const hourLabel = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString([], { hour: 'numeric' })
  const n = (x: number) => x.toLocaleString()
  const plural = (x: number, one: string, many: string) => `${n(x)} ${x === 1 ? one : many}`
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { weekday: 'long', hour: 'numeric', minute: '2-digit' })

  /** "3 new" or "2 fewer" after a count, when it changed. */
  const change = (x: number) => (x > 0 ? ` (${n(x)} new)` : x < 0 ? ` (${n(-x)} fewer)` : '')

  function lines(s: CheckSummary): string[] {
    return [
      s.added || s.removed
        ? `${plural(s.added, 'song', 'songs')} added, ${plural(s.removed, 'song', 'songs')} gone`
        : 'No songs added or gone',
      `${plural(s.trouble, 'problem', 'problems')} needing a look${change(s.trouble_change)}`,
      `${plural(s.duplicates, 'song', 'songs')} with copies${change(s.duplicates_change)}`,
      `${plural(s.reports, 'song', 'songs')} listeners reported${change(s.reports_change)}`,
    ]
  }
</script>

<section class="card">
  <h2>Nightly check</h2>
  {#if check}
    <label class="switch big">
      <input type="checkbox" bind:checked={check.on} onchange={save} />
      Check the library every night
    </label>
    <label>
      At
      <select bind:value={check.hour} onchange={save} disabled={!check.on}>
        {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
          <option value={h}>{hourLabel(h)}</option>
        {/each}
      </select>
      <span class="small muted">
        This computer's time. DJINN notices most changes to your folders right away; a nightly check also catches
        network folders and drives that don't say when they change. If DJINN is off at that hour, the check runs when
        it starts.
      </span>
    </label>
    {#if check.last}
      <div>
        <p class="small muted">Last check, {when(check.last.at)}:</p>
        <ul class="small">
          {#each lines(check.last) as line, i (i)}<li>{line}</li>{/each}
        </ul>
      </div>
    {/if}
  {/if}
</section>

<style>
  ul {
    margin: 4px 0 0;
    padding-left: 20px;
  }
</style>
