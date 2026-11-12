# Troubleshooting

## Phones can't connect

- **Same network:** phones must be on the same Wi-Fi as the DJINN computer,
  not a guest network or mobile data.
- **Windows network type:** if Windows calls your network **Public**, it
  blocks guests. DJINN shows a note about this on the join screen and in
  Admin. Switch the network to **Private** in Windows Settings > Network &
  internet, and allow DJINN on private networks if Windows asks.
- **VPN:** a VPN on the DJINN computer can hide it from the network. Turn it
  off or allow local network access.
- **VPN on the phone:** a VPN app on the phone can keep it from reaching
  devices at home. Turn it off while at the party.
- **Android and Chrome (ERR_TOO_MANY_RETRIES):** newer Android asks before
  an app may reach devices on your home network. In the phone's Settings >
  Apps > Chrome > Permissions, allow **Nearby devices** (some phones call it
  Local network), then reload. Other browsers may work without it until
  they ask too.
- **The address:** the join QR code and the address under it point at this
  computer. They follow it when the router gives it a new address (see
  below). Admin > System > **Status** lists every address phones can use.

## The computer's address changed

Routers hand out addresses, and can give the DJINN computer a new one after
a restart or a power cut, or when it moves from Wi-Fi to a cable. DJINN
notices within about 10 seconds and carries on:

- **Join links and QR codes** show the new address, on open pages and the TV
  page too. Guests who joined before scan the code again.
- **Casts** are started again with the new address.
- **A box's own screen** (the install discs) shows the new address.
- **Nodes** look for their main DJINN after 15 seconds without it: on the
  network, at addresses it had before, and by its name. They only follow the
  one with the certificate they paired with.
- **DJINN GO for Android** looks for the server on the network when it can't
  reach it, and follows it if it's the same server (encrypted connections
  only).
- For an hour after a change, DJINN announces itself on the network even if
  **Let phones on this network find DJINN** is off, so phones and nodes
  that knew the old address can find it.

Web GO in a browser and saved bookmarks still have the old address: open the
new one from Admin > System > **Status**, or use the name instead (below).

### Keep the same address

The most reliable fix is an address that never changes:

- **Reserve it on the router.** Most routers have a page called DHCP
  reservation, address reservation or static lease: pick the DJINN computer
  from the list of devices and keep its current address. Nothing changes on
  the computer itself. Do the same for each node.
- **Use its name.** On most networks the computer also answers at
  `http://<name>.local:8066`, listed last under Admin > System > **Status**
  (`http://djinn.local:8066` on the install discs). Phones and laptops can bookmark
  that instead of the numbers. Some Android phones and some routers don't
  resolve these names; the reserved address always works.
- **Tailscale.** If the DJINN computer and your phones run
  [Tailscale](https://tailscale.com), its Tailscale address (`100.x.y.z`)
  stays the same whatever the router does, at home and away. See
  [Using GO away from home](djinn-go.md#tailscale-recommended). Nodes need
  to be on the same home network as their main DJINN.

## No sound

- The Playing tab shows "Audio output problem" or "No audio output yet" when
  DJINN can't play. Check Admin > Audio > **Outputs**: is at least one output
  **On**?
- A Bluetooth speaker has to be paired and connected to the computer before
  it shows up under **Add an output**.
- Check the computer's own volume and the volume on the Playing tab.
- Casting? Network devices run a few seconds behind; see
  [Casting, Play here and the TV page](casting-and-tv.md).

## Speakers are out of time

Raise the **Delay** on the outputs that play early (usually the wired ones,
since Bluetooth lags) until they line up. See
[Speakers and sound](speakers-and-sound.md).

## Songs are missing

- Is their folder listed under Admin > Library? A folder marked "Not found
  right now" is unplugged or offline.
- Tap **Check for changes**.
- DJINN reads MP3, FLAC, AAC/M4A, Ogg Vorbis and WAV. Other formats are
  skipped.
- One artist split into two, or two artists merged? See
  [Artist names that are never split](your-library.md#artist-names-that-are-never-split).

## The computer is busy after adding a lot of music

Some background work is normal after a big library change: DJINN reads each
new song, and (when they're switched on) analyzes how it sounds and lets the
AI listen. Admin > Library shows each step and how far along it is.

On Windows, **Windows Security** also checks every song the first time
DJINN reads it, and can use more of the processor than DJINN itself during
the first library check. It settles down once the check is done. To skip
that check for your music, add the music folders as exclusions in Windows
Security > Virus & threat protection > Manage settings > Exclusions (this
needs an administrator). Don't exclude the folder for submitted music if
you accept uploads from other people: those files should still be checked.

## Windows says "Windows protected your PC"

DJINN isn't code signed. Click **More info**, then **Run anyway**.

## The installer does nothing, or Windows blocks it with no way around

That's **Smart App Control**, which Windows 11 switches on for new
installs. It blocks programs that aren't code signed, DJINN included, and
unlike the warning above it has no **Run anyway**. Check Windows Security >
App & browser control > Smart App Control.

Your choices:

- **Turn it off** in that same place, then run the installer again. Windows
  can't turn Smart App Control back on afterwards without resetting the PC,
  so decide with that in mind. Windows Security and Defender keep working.
- **Run DJINN on another computer**: one where Smart App Control is off, or
  Linux.

## The admin password is lost

Another admin can reset it under Admin > Users > **Reset password**.

## Still stuck

Admin > System > **Log** shows what DJINN has been doing; pick **Errors** to
see what went wrong. Admin > About > **Report a problem** opens an issue on
GitHub; attach the log from **Download the log**, or choose an earlier day
under **Daily logs**. Dated files such as `djinn-2026-11-01.log` are next
to `djinn.exe` in `%LOCALAPPDATA%\Programs\DJINN` on Windows, and in
`~/.config/djinn/` on Linux.
