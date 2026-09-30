//! `erisauth`: set up and look after home, ErisAuth's plugin in ErisDB.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use erisauth::{erisdb::Session, home};
use uuid::Uuid;

/// Where home keeps its ErisDB credential, as the shipped manifest names it.
const HOME: &str = "/var/lib/erisauth";

#[derive(Parser)]
#[command(version, about = "Sign apps in to Eris services from a device you already trust")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Home, ErisAuth's plugin in ErisDB. Run these on the ErisDB machine.
    Home {
        #[command(subcommand)]
        command: HomeCommand,
    },
}

#[derive(Subcommand)]
enum HomeCommand {
    /// Enroll home with ErisDB, register its facet, and set the passphrase.
    /// Cut the ticket with `erisdb pair` and approve the request there.
    Setup {
        /// The plugin's state directory: the `--home` in its manifest.
        #[arg(long, default_value = HOME)]
        home: PathBuf,
        /// ErisDB's loopback URL; home proves itself to the core over it.
        #[arg(long, default_value = "http://127.0.0.1:7700")]
        erisdb: String,
        /// The `erisdb://pair/…` ticket.
        ticket: String,
    },
    /// Set a new passphrase without the old one.
    Passphrase {
        #[arg(long, default_value = HOME)]
        home: PathBuf,
    },
    /// Let a device, or every device, guess the passphrase again now.
    Unlock {
        #[arg(long, default_value = HOME)]
        home: PathBuf,
        /// The device's ErisDB installation id; every device when omitted.
        installation: Option<Uuid>,
    },
}

/// A new passphrase: typed twice at a terminal, or one line of piped input.
fn new_passphrase() -> Result<String> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    loop {
        let first = rpassword::prompt_password("New passphrase: ")?;
        if first.chars().count() < home::MIN_PASSPHRASE {
            eprintln!("A passphrase has at least {} characters.", home::MIN_PASSPHRASE);
            continue;
        }
        if rpassword::prompt_password("Again: ")? == first {
            return Ok(first);
        }
        eprintln!("Those differ; once more.");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Home { command } => match command {
            HomeCommand::Setup { home: dir, erisdb, ticket } => {
                home::enroll(&dir, &erisdb, &ticket, |line| println!("{line}"))
                    .await
                    .context("enrolling home with ErisDB")?;
                let mut db = Session::open(&dir)?;
                home::register(&mut db).await?;
                let passphrase = new_passphrase()?;
                home::set_passphrase(&mut db, &passphrase).await?;
                println!("Home is set up in {}.", dir.display());
            }
            HomeCommand::Passphrase { home: dir } => {
                let mut db = Session::open(&dir)?;
                let passphrase = new_passphrase()?;
                home::set_passphrase(&mut db, &passphrase).await?;
                println!("The passphrase is changed.");
            }
            HomeCommand::Unlock { home: dir, installation } => {
                let mut db = Session::open(&dir)?;
                if home::load(&mut db).await?.is_none() {
                    bail!("home has no passphrase yet; run `erisauth home setup`");
                }
                home::unlock(&mut db, installation).await?;
                match installation {
                    Some(id) => println!("{id} may guess again."),
                    None => println!("Every device may guess again."),
                }
            }
        },
    }
    Ok(())
}
