# Accounts and roles

Everything here is under Admin > **Users**. The same accounts work for the
party and for [DJINN GO](djinn-go.md), your personal listening. Everyone can
change their own username and password, turn on two-step sign-in and see where they're
signed in under **Your account** (on the Playing tab).

## Roles

A role is what a kind of account can do. Every account is an **Admin** (can
do everything) or has one role. DJINN starts with one role, **Listener**,
which you can edit but not delete. Make more with **New role**, for example
"Family" that can use GO away from home, or "Friends" that can only use it on
your network.

What a role can allow:

- **Use DJINN GO:** personal listening, apart from the party.
- **Use GO away from home:** without it, GO only works on your home network.
- **GO streams at once:** how many devices can play at the same time.
- **Must use two-step sign-in for GO:** a code from an authenticator app
  after the password. See [DJINN GO](djinn-go.md#two-step-sign-in).
- **Download songs:** save song files to a device. Off unless you turn it on.
- **Have songs converted:** MP3 copies when their device needs them, once
  you turn converting on. See [DJINN GO](djinn-go.md#converting).
- **Share playlists:** let other people see and play their playlists.
- **Submit music:** upload songs for the library from DJINN GO, and
  whether they go **straight in** or wait for you to review them. See
  [DJINN GO](djinn-go.md#submitting-music).
- **No party request limit:** skip the per-person limit and the queue cap,
  like marking each of them **No limit**.

Deleting a role moves its accounts to Listener.

## Adding people

There are three ways in, and you can use any of them:

- **Add them yourself:** under Accounts, type a username and password twice, pick a
  role, and tap **Add account**.
- **Invite links:** under Invite links, pick a role, how many times the link
  can be used and for how long, and tap **Make invite link**. Copy the link
  then: it isn't shown again. Send it to them; opening it lets them create
  their own account in that role. Links work even when sign-ups are closed,
  and you can cancel one any time.
- **Sign-ups:** under Sign-ups, choose who can create an account from the
  sign-in screen ("New here? Create an account"):
  - **Only people you invite or add** (the default): no sign-up button.
  - **Anyone, after you approve them:** new accounts show under **Waiting
    for approval** at the top of Admin > Users. **Approve** lets them sign
    in; **Turn down** deletes the account.
  - **Anyone, straight away:** anyone who can open DJINN can make an
    account and use it at once. Pick a role that can't do much.

  You also choose the role new sign-ups get.

Passwords need at least 8 characters, and very common ones (like
"password123") are refused. A few words together are easy to remember and
hard to guess.

## Looking after accounts

Each account has:

- **Change username:** give the account a new sign-in name. The old name stops
  working, but signed-in devices stay signed in. Phone apps need the new
  username. Everyone can also change
  their own username under **Your account** after entering their password.
- **Role:** change it from the list; pick Admin to make someone an admin.
- **No limit:** this person's party requests aren't limited.
- **Devices:** where they're signed in, with the browser, address and when
  it was last used. Sign out one device or all of them, for example after a
  lost phone.
- **Reset password:** sets a new one and signs them out everywhere.
- **Turn off two-step:** for someone who lost their phone and their
  recovery codes.
- **Turn off:** signs them out and stops them signing in, without deleting
  anything. **Turn on** undoes it.
- **×** deletes the account (their queued songs go too).

You can't turn off, delete or demote your own account, and DJINN always
keeps at least one admin.

## Protected areas

Anything that changes your music library, meaning the folders and the files in them, sits behind a lock: writing tags, renaming or moving files, and adding songs (approving a submission already counts). DJINN's own database (tag corrections kept in DJINN, history, lyrics) is not behind it. Today only approving a submission is locked; the editing screens that come later will use the same lock.

Admin > Access > Protected areas sets how strict it is:

- **Max** (the default): opening a protected area asks for your password and a code from your authenticator app. It needs two-step sign-in set up on your account (Account page), so until then an admin at Max can't open protected areas.
- **Medium**: your password only.
- **Off**: no lock. The page warns that anyone who can sign in as an admin could then change or delete your files, and asks you to type OFF.

An unlock belongs to your session. It ends when you leave Admin, when you select Lock now, after ten minutes without use, or after an hour at most. Changing the level needs your password, and leaving Max needs a code too, so a stolen password alone can't loosen the lock. The server enforces the lock, not just the screen.

**Audit trail.** Each unlock, failed try, change of level and library change is recorded with who, when and from where. In the same page you can turn the trail off and delete all records (both need the area unlocked). While the trail is on, deleting leaves one record of itself; turn it off first to leave no trace.
