# Gapless fixtures

One continuous mono 44.1 kHz signal, `0.4 * sin(440 Hz) + 0.1 * sin(1000 Hz)`,
split at frame 13337 into `a` (frames 0..13337) and `b` (13337..25337), then
encoded separately. Played back to back, the pair must reproduce the uncut
signal with no gap or click at the seam.

Regenerate (write `a.wav` and `b.wav` as 16-bit mono first):

```sh
lame --quiet -b 192 a.wav gapless_a.mp3    # writes the LAME delay/padding header
lame --quiet -b 192 b.wav gapless_b.mp3
ffmpeg -i a.wav -c:a flac gapless_a.flac
ffmpeg -i b.wav -c:a flac gapless_b.flac
```
