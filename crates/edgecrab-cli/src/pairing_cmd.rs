use std::path::PathBuf;

use anyhow::Context;
use edgecrab_gateway::pairing::PairingStore;

use crate::cli_args::PairingCommand;

pub fn run(command: PairingCommand) -> anyhow::Result<()> {
    let store = PairingStore::new();
    match command {
        PairingCommand::List {
            pending,
            approved,
            platform,
        } => list_pairings(&store, pending, approved, platform.as_deref()),
        PairingCommand::Approve { platform, code } => approve_pairing(&store, &platform, &code),
        PairingCommand::Revoke { platform, user_id } => revoke_pairing(&store, &platform, &user_id),
        PairingCommand::ClearPending { platform } => clear_pending(platform.as_deref()),
    }
}

fn list_pairings(
    store: &PairingStore,
    pending_only: bool,
    approved_only: bool,
    platform: Option<&str>,
) -> anyhow::Result<()> {
    let show_pending = pending_only || !approved_only;
    let show_approved = approved_only || !pending_only;

    if show_pending {
        let pending = store.list_pending(platform);
        if pending.is_empty() {
            println!("  Pending pairing requests: none");
        } else {
            println!("  Pending pairing requests ({}):", pending.len());
            println!(
                "  {:<12} {:<8}  {:<20}  {:<20}  Age",
                "Platform", "Code", "User ID", "Name"
            );
            println!("  {}", "-".repeat(72));
            for (pf, code, user_id, user_name, age_min) in &pending {
                println!(
                    "  {:<12} {:<8}  {:<20}  {:<20}  {}m ago",
                    pf,
                    code,
                    user_id,
                    display_name(user_name),
                    age_min
                );
            }
            println!();
            println!("  To approve: edgecrab pairing approve <platform> <code>");
            println!("  To deny all: edgecrab pairing clear-pending [--platform <p>]");
        }
    }

    if show_approved {
        if show_pending {
            println!();
        }
        let approved = store.list_approved(platform);
        if approved.is_empty() {
            println!("  Approved pairings: none");
            println!();
            println!("  Tip: Users can request pairing by messaging the bot.");
            println!("       The bot will send them a pairing code to present here.");
        } else {
            println!("  Approved pairings ({}):", approved.len());
            println!("  {:<12} {:<24}  Name", "Platform", "User ID");
            println!("  {}", "-".repeat(52));
            for (pf, user_id, user_name) in &approved {
                println!("  {:<12} {:<24}  {}", pf, user_id, display_name(user_name));
            }
            println!();
            println!("  To revoke: edgecrab pairing revoke <platform> <user_id>");
        }
    }

    Ok(())
}

fn approve_pairing(store: &PairingStore, platform: &str, code: &str) -> anyhow::Result<()> {
    match store.approve_code(platform, code) {
        Some((user_id, user_name)) => {
            println!(
                "  ✓ Approved pairing for {} user {} ({}).",
                platform,
                user_id,
                display_name(&user_name)
            );
            println!();
            println!(
                "  The user can now send messages to the bot on {}.",
                platform
            );
            println!(
                "  To revoke later: edgecrab pairing revoke {} {}",
                platform, user_id
            );
            Ok(())
        }
        None => anyhow::bail!(
            "No pending pairing code '{}' for platform '{}'.  \
             Use `edgecrab pairing list --pending` to see active requests.",
            code,
            platform
        ),
    }
}

fn revoke_pairing(store: &PairingStore, platform: &str, user_id: &str) -> anyhow::Result<()> {
    if store.revoke(platform, user_id) {
        println!("  ✓ Revoked pairing for {} user {}.", platform, user_id);
        println!("    The user will need to pair again to regain access.");
        Ok(())
    } else {
        anyhow::bail!(
            "User '{}' is not currently paired on '{}'.  \
             Use `edgecrab pairing list --approved` to see paired users.",
            user_id,
            platform
        );
    }
}

fn clear_pending(platform: Option<&str>) -> anyhow::Result<()> {
    let files = pending_files(platform)?;
    if files.is_empty() {
        println!("  No pending pairing files found.");
        return Ok(());
    }

    for path in &files {
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }

    let scope = platform.map_or("(all platforms)".to_string(), |p| p.to_string());
    println!(
        "  ✓ Cleared {} pending pairing file(s) for {}.",
        files.len(),
        scope
    );
    Ok(())
}

fn pending_files(platform: Option<&str>) -> anyhow::Result<Vec<PathBuf>> {
    let dir = edgecrab_core::edgecrab_home().join("pairing");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    if let Some(platform) = platform {
        return Ok(vec![dir.join(format!("{platform}-pending.json"))]);
    }

    let mut files = Vec::new();
    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with("-pending.json"))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn display_name(name: &str) -> &str {
    if name.trim().is_empty() { "-" } else { name }
}
