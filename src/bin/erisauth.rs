//! `erisauth`: join a device, serve apps on it, answer their requests, and
//! set up and look after home, ErisAuth's plugin in ErisDB.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use erisauth::{device, erisdb::Session, home};
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
    /// Join this device to ErisDB, with a ticket from `erisdb pair` or another device.
    Join {
        /// What to call this device; its hostname when omitted.
        #[arg(long)]
        name: Option<String>,
        /// A grant this device may approve for apps by itself; repeat for more.
        /// Everything (`*`) when omitted. Anything beyond takes the passphrase.
        #[arg(long = "ceiling")]
        ceiling: Vec<String>,
        /// The `erisdb://pair/…` ticket.
        ticket: String,
    },
    /// Listen for apps on this device and put their requests to the person.
    Serve {
        /// Do not show desktop notifications.
        #[arg(long)]
        no_notify: bool,
    },
    /// Open the approval page in the browser.
    Open,
    /// Answer apps' requests at this terminal.
    Approvals {
        /// Stop after answering one request.
        #[arg(long)]
        once: bool,
    },
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
        Command::Join { name, ceiling, ticket } => {
            let paths = device::Paths::from_env()?;
            let name = match name {
                Some(name) => name,
                None => std::fs::read_to_string("/proc/sys/kernel/hostname")
                    .map(|h| h.trim().to_string())
                    .unwrap_or_else(|_| "this device".into()),
            };
            let ceiling = if ceiling.is_empty() { vec!["*".to_string()] } else { ceiling };
            device::join(&paths, &ticket, &name, &ceiling, |line| println!("{line}")).await?;
        }
        Command::Serve { no_notify } => device::serve(&device::Paths::from_env()?, !no_notify).await?,
        Command::Open => {
            let link = device::page_link(&device::Paths::from_env()?)?;
            println!("{link}");
            device::open(&link);
        }
        Command::Approvals { once } => device::prompt::run(&device::Paths::from_env()?, once).await?,
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
