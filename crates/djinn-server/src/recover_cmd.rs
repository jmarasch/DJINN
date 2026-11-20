//! `djinn recover`: the last resort for an admin who can't sign in. It runs
//! on the computer DJINN is installed on and only helps when every admin
//! account is shut out (locked after wrong codes, turned off, or waiting).
//! It asks for the username, which must be typed exactly; three wrong tries
//! lock it for a while, kept in the database so restarting DJINN doesn't
//! clear it. It says only "not eligible" for any refusal.
//!
//! What it does: unlocks the account and makes it active, gives it a
//! temporary password that must be changed, turns its two-step off so it can
//! be set up again, and signs it out everywhere. It never opens a session or
//! shows a stored secret. The use is shown to admins as a banner and, when
//! the audit trail is on, recorded there.
//!
//! This is a speed bump with a record, not protection against someone with
//! full control of the computer: they can already read the database or swap
//! the program. The computer's own login and disk encryption are that.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Context;
use djinn_store::{recoverable, NewAudit, Store};

use crate::config::Config;

const NOT_ELIGIBLE: &str = "Not eligible.";
const PASSWORD_LETTERS: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub async fn run(config_path: Option<&Path>) -> anyhow::Result<()> {
    let (config, _) = Config::load(config_path)?;
    let store = Store::open(&config.database.path)
        .await
        .with_context(|| format!("couldn't open {}", config.database.path.display()))?;
    let admins = store.admin_accounts().await?;
    if admins.is_empty() {
        anyhow::bail!("DJINN isn't set up yet: there are no accounts to recover");
    }
    if let Some(wait_ms) = store.recovery_wait().await? {
        let minutes = (wait_ms / 60_000).max(0) + 1;
        anyhow::bail!("This command is locked after too many wrong tries. Try again in about {minutes} minutes");
    }

    println!("DJINN recovery. Only works when no admin account can sign in.");
    let username = ask("Admin username: ")?;
    let Some(target) = recoverable(&admins, &username) else {
        // Every refusal looks the same, and counts as a wrong try.
        eprintln!("{NOT_ELIGIBLE}");
        if let Some(minutes) = store.recovery_failed().await? {
            eprintln!("Too many wrong tries: this command is locked for {minutes} minutes.");
        }
        std::process::exit(1);
    };
    println!(
        "This unlocks \"{}\", sets a temporary password that must be changed, turns its two-step sign-in off,\nand signs it out everywhere.",
        target.username
    );
    if ask("Type RECOVER to go ahead: ")? != "RECOVER" {
        anyhow::bail!("Nothing was changed");
    }

    let password = crate::security::random_text(16, PASSWORD_LETTERS)?;
    let hash = crate::auth::hash_password(password.clone())
        .await
        .map_err(|e| anyhow::anyhow!("couldn't hash the password: {}", e.message()))?;
    store
        .recover_account(target.id, &target.username, &hash)
        .await?;
    let trail_on = store
        .get_setting(crate::protected::AUDIT_ON)
        .await?
        .is_none_or(|v| v != "0");
    if trail_on {
        store
            .log_audit(&NewAudit {
                user_id: Some(target.id),
                username: &target.username,
                ip: None,
                kind: "recovery_used",
                detail: "the host's recovery command",
            })
            .await?;
    }
    println!();
    println!(
        "Done. Sign in as \"{}\" with this temporary password:",
        target.username
    );
    println!();
    println!("    {password}");
    println!();
    println!(
        "It is shown once. Change it right after signing in, and set two-step sign-in up again."
    );
    Ok(())
}

fn ask(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}
