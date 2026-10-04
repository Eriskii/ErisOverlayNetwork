//! `erisauth`: join a device, serve apps on it, answer their requests, and
//! look after the passphrase.

use anyhow::Result;
use clap::{Parser, Subcommand};
use erisauth::device;
use uuid::Uuid;

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
    /// Change the passphrase, typing the current one; with `--reset`, set it
    /// without, which this device needs `erisauth:reset` for. That is how the
    /// first passphrase is set.
    Passphrase {
        #[arg(long)]
        reset: bool,
    },
    /// Let a device, or every device, guess the passphrase again now. This
    /// device needs `erisauth:unlock`.
    Unlock {
        /// The device's ErisDB installation id; every device when omitted.
        installation: Option<Uuid>,
    },
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
        Command::Passphrase { reset } => device::prompt::passphrase(&device::Paths::from_env()?, reset).await?,
        Command::Unlock { installation } => device::prompt::unlock(&device::Paths::from_env()?, installation).await?,
    }
    Ok(())
}
