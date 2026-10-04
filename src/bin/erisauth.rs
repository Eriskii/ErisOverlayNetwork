//! `erisauth`: join a device, serve apps on it, and answer their requests;
//! and, on the ErisDB machine, set up and look after ErisAuth's plugin there.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use erisauth::plugin::state;
use erisauth::{device, erisdb::Session};
use uuid::Uuid;

/// Where the plugin keeps its ErisDB credential, as the shipped manifest names it.
const DIR: &str = "/var/lib/erisauth";

#[derive(Parser)]
#[command(version, about = "Sign apps in to Eris services from a device you already trust")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Join this device to ErisDB, with a ticket from `erisdb pair` or from
    /// `erisauth add` on a device that has joined.
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
    /// Show a ticket another device joins with, and approve it here.
    Add,
    /// Open the approval page in the browser.
    Open,
    /// Answer apps' requests at this terminal.
    Approvals {
        /// Stop after answering one request.
        #[arg(long)]
        once: bool,
    },
    /// On the ErisDB machine: enroll ErisAuth's plugin with ErisDB, register
    /// its facet, and set the passphrase. Cut the ticket with `erisdb pair`
    /// and approve the request there.
    Setup {
        /// The plugin's state directory: the `--dir` in its manifest.
        #[arg(long, default_value = DIR)]
        dir: PathBuf,
        /// ErisDB's loopback URL; the plugin proves itself to the core over it.
        #[arg(long, default_value = "http://127.0.0.1:7700")]
        erisdb: String,
        /// The `erisdb://pair/…` ticket.
        ticket: String,
    },
    /// On the ErisDB machine: set a new passphrase without the old one.
    Passphrase {
        #[arg(long, default_value = DIR)]
        dir: PathBuf,
    },
    /// On the ErisDB machine: let a device, or every device, guess the passphrase again now.
    Unlock {
        #[arg(long, default_value = DIR)]
        dir: PathBuf,
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
        if first.chars().count() < state::MIN_PASSPHRASE {
            eprintln!("A passphrase has at least {} characters.", state::MIN_PASSPHRASE);
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
            let asking = device::begin_join(&paths, &ticket, &name, &ceiling).await?;
            println!("Compare this code with the approving screen: {}", asking.fingerprint);
            println!("{} asks to approve apps for: {}", device::installation_name(&name), ceiling.join(", "));
            let (_, granted) = asking.finish(&paths).await?;
            println!("Joined. This device holds: {}", granted.join(", "));
        }
        Command::Serve { no_notify } => device::serve(&device::Paths::from_env()?, !no_notify).await?,
        Command::Add => device::prompt::add(&device::Paths::from_env()?).await?,
        Command::Open => {
            let link = device::page_link(&device::Paths::from_env()?)?;
            println!("{link}");
            device::open(&link);
        }
        Command::Approvals { once } => device::prompt::run(&device::Paths::from_env()?, once).await?,
        Command::Setup { dir, erisdb, ticket } => {
            state::enroll(&dir, &erisdb, &ticket, |line| println!("{line}"))
                .await
                .context("enrolling ErisAuth with ErisDB")?;
            let mut db = Session::open(&dir)?;
            state::register(&mut db).await?;
            let passphrase = new_passphrase()?;
            state::set_passphrase(&mut db, &passphrase).await?;
            println!("ErisAuth is set up in ErisDB, with its state in {}.", dir.display());
        }
        Command::Passphrase { dir } => {
            let mut db = Session::open(&dir)?;
            let passphrase = new_passphrase()?;
            state::set_passphrase(&mut db, &passphrase).await?;
            println!("The passphrase is changed.");
        }
        Command::Unlock { dir, installation } => {
            let mut db = Session::open(&dir)?;
            if state::load(&mut db).await?.is_none() {
                bail!("ErisAuth has no passphrase yet; run `erisauth setup`");
            }
            state::unlock(&mut db, installation).await?;
            match installation {
                Some(id) => println!("{id} may guess again."),
                None => println!("Every device may guess again."),
            }
        }
    }
    Ok(())
}
