//! `edgecrab secret` — resolve / store secrets via SecretResolver (gap 032).

use crate::cli_args::SecretCommand;
use edgecrab_security::default_resolver;

pub fn run_secret(command: SecretCommand) -> anyhow::Result<()> {
    let home = edgecrab_core::edgecrab_home();
    let resolver = default_resolver(&home);
    match command {
        SecretCommand::List => {
            let names = resolver.list_names();
            if names.is_empty() {
                println!("(no file-backed secrets; env secrets are not listed)");
            } else {
                for n in names {
                    println!("{n}");
                }
            }
        }
        SecretCommand::Get { name } => {
            if resolver.resolve(&name).is_some() {
                println!("set");
            } else {
                println!("missing");
            }
        }
        SecretCommand::Set { name, value } => {
            resolver.set(&name, &value)?;
            println!("stored {name} under {}/secrets/", home.display());
        }
    }
    Ok(())
}
