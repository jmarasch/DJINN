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
- **The address:** the join QR code and the address under it point at this
  computer. If it changed (a new network), reopen the QR code. Admin >
  System > **Status** lists every address phones can use.

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
  [Artist names that are never split](rules.md#artist-names-that-are-never-split).

## Windows says "Windows protected your PC"

The installer isn't code signed yet. Click **More info**, then **Run anyway**.

## The admin password is lost

Another admin can reset it under Admin > Users > **Reset password**.

## Still stuck

Admin > System > **Log** shows what DJINN has been doing; pick **Errors** to
see what went wrong. Admin > About > **Report a problem** opens an issue on
GitHub; attach the log from **Download the log**. The log file itself
(`djinn.log`) is next to `djinn.exe` in `%LOCALAPPDATA%\Programs\DJINN` on
Windows, and in `~/.config/djinn/` on Linux.
