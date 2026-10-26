# Phase 3 check: several outputs in sync

Goal from the spec: the optical output and a Bluetooth speaker play together
for 3+ hours without audible drift, after calibration.

## Set up

1. Pair and connect the Bluetooth speaker to the DJINN computer (KDE's
   Bluetooth settings). It must show up as a sound device.
2. Admin > Audio > Add an output, pick the speaker.
3. Keep "System default" on (that's the optical output while it's the
   system default device).

## Calibrate

Bluetooth plays a few hundred milliseconds late, so the wired output gets
the delay.

1. Play something with sharp beats. Stand where you hear both.
2. On "System default", raise Delay until the echo closes up. Use the
   slider to get close, then the -10 / +10 buttons. Typical values are
   150 to 300 ms.
3. When the beats sound like one, it's done. The setting is saved.

## Run it

Play for 3+ hours (a long playlist on shuffle). Check now and then:

- The two never drift apart audibly.
- Admin > Audio shows the speaker "In sync", with a speed figure that
  settles at some small steady number (tens of ppm is normal; it's the
  speaker's clock being slightly off).
- Turn the speaker off and on again mid-song: the optical output keeps
  playing, the speaker reconnects on its own within a few seconds and
  joins in time.

## Volume leveling

Admin > Audio > Volume leveling > "Even out every song". DJINN measures
songs without ReplayGain tags once (a few minutes for a big library) and
keeps the result in its own database. Leveled songs play about 10 dB
quieter than modern masters; use Boost (+6 dB or so) to taste.
