//! `erisauth`: run ErisAuth on this device, and look after the mesh from a terminal.

use anyhow::Result;
use clap::{Parser, Subcommand};
use erisauth::device::{self, Paths, prompt};
use erisoverlaynetwork::net::Network;

#[derive(Parser)]
#[command(version, about = "The core of each device in an Eris mesh: its key, its devices, and the apps it approves")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run ErisAuth: the device's node, the agent socket, the approval page, notifications.
    Serve {
        /// Do not show desktop notifications.
        #[arg(long)]
        no_notify: bool,
        /// How to reach the other devices: `internet`, `loopback`, or a relay URL. `ERIS_NETWORK`
        /// says when this does not; the internet when neither does.
        #[arg(long)]
        network: Option<Network>,
    },
    /// Start a new mesh, with this device as its first member.
    Init {
        /// What to call this device; its hostname when omitted.
        #[arg(long)]
        name: Option<String>,
    },
    /// Join the mesh of the device showing a ticket (`erisauth add` there).
    Join {
        /// What to call this device; its hostname when omitted.
        #[arg(long)]
        name: Option<String>,
        /// The `eris://pair/…` ticket.
        ticket: String,
    },
    /// Show a ticket a new device joins with, and approve it here.
    Add,
    /// Show a ticket an app with no ErisAuth beside it (a browser app) asks with, and approve it here.
    AddApp,
    /// Answer what apps and new devices ask, at this terminal.
    Approvals {
        /// Stop after answering one request.
        #[arg(long)]
        once: bool,
    },
    /// List the mesh's devices.
    Devices,
    /// Call a device something new.
    Rename {
        /// The device: its name, or the start of its id.
        device: String,
        name: String,
    },
    /// Remove a device from the mesh for good, with the devices it added.
    Remove {
        /// The device: its name, or the start of its id.
        device: String,
        /// A device it added to keep; repeat for more.
        #[arg(long)]
        keep: Vec<String>,
        /// Remove without asking.
        #[arg(long)]
        yes: bool,
    },
    /// List the apps this device approved.
    Apps,
    /// Revoke an app's approval, everywhere.
    Revoke {
        /// The app's name, or the start of its note's id.
        app: String,
    },
    /// Change ErisDB's passphrase, typing the current one.
    Passphrase,
    /// Open the approval page in the browser.
    Open,
}

#[tokio::main]
async fn main() -> Result<()> {
    let paths = Paths::from_env()?;
    match Cli::parse().command {
        Command::Serve { no_notify, network } => {
            device::serve(&paths, network.unwrap_or_else(Network::from_env), !no_notify).await?
        }
        Command::Init { name } => prompt::init(&paths, &name.unwrap_or_else(prompt::hostname)).await?,
        Command::Join { name, ticket } => prompt::join(&paths, &name.unwrap_or_else(prompt::hostname), &ticket).await?,
        Command::Add => prompt::add(&paths, false).await?,
        Command::AddApp => prompt::add(&paths, true).await?,
        Command::Approvals { once } => prompt::approvals(&paths, once).await?,
        Command::Devices => prompt::devices(&paths).await?,
        Command::Rename { device, name } => prompt::rename(&paths, &device, &name).await?,
        Command::Remove { device, keep, yes } => prompt::remove(&paths, &device, &keep, yes).await?,
        Command::Apps => prompt::apps(&paths).await?,
        Command::Revoke { app } => prompt::revoke(&paths, &app).await?,
        Command::Passphrase => prompt::passphrase(&paths).await?,
        Command::Open => {
            let link = device::page_link(&paths)?;
            println!("{link}");
            device::open(&link);
        }
    }
    Ok(())
}
