<script lang="ts">
  // "Report a problem" with a song: a reason and an optional note, for the
  // admin's Problems list. Used by the party app and DJINN GO.
  import { t } from './i18n.svelte'
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
      toast(t('menu.report_thanks'))
      onclose()
    } catch (err) {
      toast(err instanceof Error ? err.message : t('wish.failed'), true)
    } finally {
      sending = false
    }
  }
</script>

<Sheet title={about === 'lyrics' ? t('rs2.report_lyrics') : t('rs2.report')} {onclose}>
  <form onsubmit={send}>
    <p class="small muted">
      {about === 'lyrics' ? t('rs2.whats_wrong_lyrics', { title: song.title }) : t('menu.whats_wrong', { title: song.title })}
    </p>
    {#each reasons as r (r.id)}
      <label class="switch choice">
        <input type="radio" name="reason" value={r.id} bind:group={reason} />
        {r.label}
      </label>
    {/each}
    <label>
      {t('rs2.anything')}
      <textarea rows="3" maxlength="300" bind:value={note}></textarea>
    </label>
    <button class="primary" disabled={!reason || sending}>{t('common.send')}</button>
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
