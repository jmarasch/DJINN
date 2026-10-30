# Back up and restore

Admin > **System** > **Back up and restore** saves everything DJINN knows
to one file, and puts it back, on this computer or a new one.

A backup holds your settings, accounts, playlists, play and request history,
party styles, speakers and casts, tag corrections, song analysis, and the
artwork DJINN collected. It doesn't hold your music, the AI models (DJINN
downloads them again when you turn AI back on), or `config.toml`, which
belongs to the computer.

Keep backups private: they hold your accounts (passwords are stored
scrambled, not as text).

## Make a backup

Tap **Download a backup**. Your browser saves a file named like
`djinn-backup-2026-10-29.zip`. DJINN keeps playing while it's made.

## Restore a backup

1. Tap **Restore from a backup…** and pick the backup file.
2. DJINN shows when the backup was made and the music folders it knew. For
   each folder, say where that music is on this computer: type the folder or
   tap **Browse**. A folder that exists here is filled in already. Leave a
   folder empty if its music isn't on this computer; its songs show as
   missing.
3. Tap **Restore and restart**. DJINN restarts with the backup in place and
   checks the music folders.
4. Sign in with an account from the backup.

Restoring replaces everything DJINN knows on this computer. What was there is
kept as `djinn.db.before-restore` next to DJINN's database, in case you need
it back. If the backup's database can't be opened, DJINN goes back to the one
it had and nothing changes.

A backup made by a newer DJINN can't be restored by an older one;
[update](updates.md) first.

## Moving to a new computer

1. On the old computer, download a backup.
2. Install DJINN on the new computer
   ([Getting started](getting-started.md)) and copy your music over.
3. Restore the backup there, pointing each music folder at where you put the
   music. Windows and Linux paths both work, so you can move between them.
