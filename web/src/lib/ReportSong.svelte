<script lang="ts">
  // "Report a problem" with a song: a reason and an optional note, for the
  // admin's Problems list. Used by the party app and DJINN GO.
  import { api } from './api'
  import { reportReasons, type ReportReason } from './reports'
  import Sheet from './Sheet.svelte'
  import { toast } from './state.svelte'

  let { song, onclose }: { song: { id: number; title: string }; onclose: () => void } = $props()

  let reason = $state<ReportReason | null>(null)
  let note = $state('')
  let sending = $state(false)

  async function send(e: SubmitEvent) {
    e.preventDefault()
    if (!reason) return
    sending = true
    try {
      await api.reportSong(song.id, reason, note.trim() || undefined)
      toast('Thanks, the admin will take a look')
      onclose()
    } catch (err) {
      toast(err instanceof Error ? err.message : 'Could not send it', true)
    } finally {
      sending = false
    }
  }
</script>

<Sheet title="Report a problem" {onclose}>
  <form onsubmit={send}>
    <p class="small muted">What's wrong with "{song.title}"?</p>
    {#each reportReasons as r (r.id)}
      <label class="choice">
        <input type="radio" name="reason" value={r.id} bind:group={reason} />
        {r.label}
      </label>
    {/each}
    <label>
      Anything else the admin should know (optional)
      <textarea rows="3" maxlength="300" bind:value={note}></textarea>
    </label>
    <button class="primary" disabled={!reason || sending}>Send</button>
  </form>
</Sheet>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
