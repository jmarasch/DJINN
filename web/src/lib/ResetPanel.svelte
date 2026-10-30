<script lang="ts">
  // Admin > System: clearing things out. Each one asks first, and each
  // says to back up before going ahead, since none can be undone.
  import { api } from './api'
  import { reloadAfterRestart } from './restart'
  import { toast } from './state.svelte'
  import type { SystemReset } from './types'

  const resets: { what: SystemReset; label: string; about: string; confirm: string }[] = [
    {
      what: 'queue',
      label: 'Clear the queue',
      about: 'Removes every song waiting to play. The song playing now finishes.',
      confirm: 'Remove every song from the queue?',
    },
    {
      what: 'tag_edits',
      label: 'Undo all tag corrections',
      about: 'Every song, album and artist goes back to what its files say.',
      confirm: 'Undo every tag correction made in DJINN?',
    },
    {
      what: 'analysis',
      label: 'Analyze all songs again',
      about:
        'Forgets how every song sounds and how loud it is, then measures them again in the background. "More like this" and radio work less well until it finishes.',
      confirm: 'Forget the analysis of every song and start it again? DJINN restarts.',
    },
    {
      what: 'listening',
      label: 'Let the AI listen again',
      about: 'Forgets what the AI heard in every song and listens to them all again, which takes a while.',
      confirm: 'Forget what the AI heard and listen to every song again? DJINN restarts.',
    },
    {
      what: 'history',
      label: 'Clear play and request history',
      about:
        'Forgets every play and request: top songs, the request log, and what the auto mix learned for every party style.',
      confirm: 'Forget every play and request, and what the auto mix learned? DJINN restarts.',
    },
    {
      what: 'settings',
      label: 'Reset settings',
      about:
        'Puts the rules, sound, smart play, AI and update options back to their defaults. Accounts, music folders, playlists, speakers, casts and online service details stay.',
      confirm: 'Put every setting back to its default? DJINN restarts.',
    },
  ]

  let busy = $state<SystemReset | null>(null)
  let restarting = $state(false)

  $effect(() => {
    if (restarting) return reloadAfterRestart()
  })

  async function run(reset: (typeof resets)[number]) {
    if (!confirm(`${reset.confirm}\n\nThis can't be undone. Download a backup first if you might want it back.`))
      return
    busy = reset.what
    try {
      const done = await api.systemReset(reset.what)
      if (done.restarting) restarting = true
      else toast('Done')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    } finally {
      busy = null
    }
  }
</script>

<section class="card">
  <h2>Clear and reset</h2>
  {#if restarting}
    <p class="small"><span class="spinner" aria-hidden="true"></span> Done. DJINN is restarting; this page reloads when it's back.</p>
  {:else}
    {#each resets as reset (reset.what)}
      <div class="reset">
        <p class="small">{reset.about}</p>
        <button class="danger" disabled={busy !== null} onclick={() => run(reset)}>{reset.label}</button>
        <p class="small muted note">
          This can't be undone. Back up DJINN first: <a href="/api/backup" download>download a backup</a>.
        </p>
      </div>
    {/each}
  {/if}
</section>

<style>
  .reset {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 12px 0;
    border-top: 1px solid var(--line);
  }

  .reset:first-of-type {
    border-top: 0;
    padding-top: 0;
  }

  p {
    margin: 0;
  }
</style>
