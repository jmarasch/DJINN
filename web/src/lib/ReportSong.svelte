<script lang="ts">
  // "Report a problem" with a song: a reason and an optional note, for the
  // admin's Problems list. Used by the party app and DJINN GO.
  import { api } from './api'
  import { lyricsReasons, reportReasons, type ReportReason } from './reports'
  import Sheet from './Sheet.svelte'
  import { toast } from './state.svelte'

  let {
    song,
    onclose,
    about = 'song',
  }: { song: { id: number; title: string }; onclose: () => void; about?: 'song' | 'lyrics' } = $props()
  const reasons = $derived(about === 'lyrics' ? lyricsReasons : reportReasons)

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

<Sheet title={about === 'lyrics' ? 'Report the lyrics' : 'Report a problem'} {onclose}>
  <form onsubmit={send}>
    <p class="small muted">
      {about === 'lyrics' ? `What's wrong with the lyrics of "${song.title}"?` : `What's wrong with "${song.title}"?`}
    </p>
    {#each reasons as r (r.id)}
      <label class="switch choice">
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
    justify-content: flex-start;
    text-align: left;
  }
</style>
