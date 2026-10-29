# Guests and requests

## Joining

Guests scan the join QR code (or type the address under it) on the same
network as DJINN. What happens next depends on
[guest mode](rules.md#guest-mode):

- **Guest mode on:** they type a name and they're in, no account needed. The
  name shows next to the songs they add. Next time, the same phone is welcomed
  back by name.
- **Guest mode off:** they sign in with an account you made for them under
  Admin > Users.

Hosts sign in with **Host sign in** at the bottom of the guest screen.

## Requesting a song

Guests search or browse the **Library** and tap a song to add it to the
queue. Requests play in order, before anything from the auto mix.

- **Taking one back:** on the Queue tab, tap × next to your own song.
- **Backing a song:** if someone already queued the song you want, tap the
  up arrow next to it on the Queue tab ("Ask for it too"). It uses one of your
  requests, and songs more people want play first.
- **Limits:** each person can have a few songs queued at once (3 by default)
  and the whole queue can be capped. See [Rules](rules.md). Admins, and people
  you mark **No limit** under Admin > Users, aren't limited.

The Playing tab shows who asked for the current song ("Requested by...") and
what's up next.

## Skipping

- **Admins** skip right away with the skip button.
- **Whoever requested a song** can always skip it alone ("Skip my song").
- **Everyone else** can vote, if [skip votes](rules.md#skip-votes) are on: the
  song is skipped once enough people tap **Vote to skip**. After voting on one
  song, you sit out the vote on the next.

Skips teach [smart play](smart-play.md) what the party doesn't want.

## Managing the queue (admins)

On the Queue tab admins can remove any request and move songs up or down.
Songs with the same number of backers can be reordered; a song more people
asked for stays ahead.

Admin > **Log** lists every request with who made it and from which device.
**Clear** empties the log; artist pages keep counting their top songs.

## Accounts

Under Admin > **Users**:

- **Accounts:** add users or admins, reset passwords, change roles, and give
  someone **No limit**.
- **Guests:** everyone who joined with just a name, by device (recognized by
  its network address), with how many songs they requested. Give a guest **No
  limit**, or remove them (their queued songs go too).
