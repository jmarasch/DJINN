# Phase 5 check: casting

Goal from the spec: a Chromecast, a DLNA TV or receiver, and a Sonos
speaker play the party at the same time for a full evening, with skips and
song changes reaching them.

## Try it

1. Admin > Audio > Cast to TVs and speakers. DJINN looks for devices on
   the Wi-Fi when the page opens; "Look for devices" looks again.
2. Press Cast on a TV. Within a few seconds it shows DJINN with the join QR
   code and plays the party. The playing screen on phones is unchanged.
3. Cast to a second device, and a speaker group (Google Home groups stay in
   sync with each other).
4. Skip a song: the casts follow a few seconds later (each device keeps a
   few seconds of audio in hand).
5. Pause: the casts go quiet but stay connected. Resume: they carry on.
6. Turn a TV off and on again mid-song: DJINN reconnects within about ten
   seconds on its own.
7. Restart DJINN: the casts that were running start again.

## Notes

- Casting takes over whatever the device was doing (Netflix, Spotify).
- Network devices run a few seconds behind this computer's speakers and
  can't be synced to them. Use casts or local speakers in a given room.
- With every local output off, DJINN keeps time by itself and plays to the
  casts alone.
- TVs set their own volume ("Volume is set on the TV"); Nest speakers and
  Sonos get a slider.
- The TV page has "Play sound here" for TVs and tablets with a browser.
