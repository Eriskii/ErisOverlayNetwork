//! ErisAuth on a device.
//!
//! A device joins ErisDB once, as an installation of its own over Iroh
//! ([`join`]). It asks for the right to open and approve pairings, to ask
//! home, and for a ceiling: the grants it may approve for an app by itself.
//! [`serve`] then listens where apps look for it and puts their requests
//! to the person.

pub mod agent;
pub mod notify;
pub mod page;
pub mod prompt;
pub mod socket;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// What a device holds besides its ceiling.
pub const BASE: [&str; 5] =
    ["meta:pairing:create", "meta:pairing:read", "meta:pairing:approve", "erisauth:approve", "erisauth:passphrase"];

/// Where a device keeps its key and its standing with ErisDB, and where it listens.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `$XDG_DATA_HOME/erisauth`.
    pub data: PathBuf,
    /// `$XDG_RUNTIME_DIR`, under which [`erislogin::agent::socket_in`] puts the socket.
    pub runtime: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let data = var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".local/share")))
            .context("neither XDG_DATA_HOME nor HOME is set")?
            .join("erisauth");
        let runtime = var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set; ErisAuth listens there")?;
        Ok(Paths { data, runtime })
    }

    pub fn key(&self) -> PathBuf {
        self.data.join("key")
    }

    pub fn device(&self) -> PathBuf {
        self.data.join("device.json")
    }

    pub fn socket(&self) -> PathBuf {
        erislogin::agent::socket_in(&self.runtime)
    }
}

/// This device's standing with ErisDB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joined {
    /// The device's name, as the person gave it.
    pub name: String,
    /// ErisDB's Iroh endpoint id.
    pub eid: String,
    /// ErisDB's direct addresses, as its ticket named them.
    #[serde(default)]
    pub addrs: Vec<SocketAddr>,
    /// The latest access token. It names the installation, which renews it
    /// by proving this device's key.
    pub token: String,
}

impl Joined {
    pub fn load(paths: &Paths) -> Result<Option<Self>> {
        match std::fs::read(paths.device()) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).context("device.json is damaged")?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        crate::erisdb::write_private(&paths.device(), &serde_json::to_vec_pretty(self)?)
    }

    /// The name this device's installation goes by in ErisDB.
    pub fn installation(&self) -> String {
        installation_name(&self.name)
    }

    pub fn addr(&self) -> Result<iroh::EndpointAddr> {
        let id: iroh::EndpointId = self.eid.parse().context("device.json names a damaged endpoint id")?;
        Ok(iroh::EndpointAddr::from_parts(id, self.addrs.iter().copied().map(iroh::TransportAddr::Ip)))
    }

    /// Dial ErisDB as this device.
    pub async fn dial(&self, paths: &Paths) -> Result<erisdb_client::Client> {
        let key = erislogin::key::load_or_create(&paths.key())?;
        erisdb_client::Client::dial_addr(self.addr()?, &self.token, &self.installation(), Some(key.to_bytes())).await
    }
}

pub fn installation_name(name: &str) -> String {
    format!("ErisAuth on {name}")
}

/// Join the ErisDB a ticket names, asking for [`BASE`] and `ceiling`.
/// `said` hears what to show the person while someone approves.
pub async fn join(
    paths: &Paths,
    ticket: &str,
    name: &str,
    ceiling: &[String],
    mut said: impl FnMut(&str),
) -> Result<Joined> {
    let ticket = erislogin::ticket::Ticket::parse("erisdb", ticket)?;
    let addr = ticket.endpoint_addr().context("this ticket has no endpoint id; ErisAuth reaches ErisDB over Iroh")?;
    let key = erislogin::key::load_or_create(&paths.key())?;
    let installation = installation_name(name);
    let client =
        erisdb_client::Client::dial_addr(addr.clone(), &ticket.token, &installation, Some(key.to_bytes())).await?;
    let requested: Vec<&str> = BASE.iter().copied().chain(ceiling.iter().map(String::as_str)).collect();
    let session = client.redeem_pairing(&installation, &requested).await?;
    let fingerprint = session["body"]["fingerprint"].as_str().context("ErisDB sent no fingerprint")?;
    said(&format!("Compare this code with the approving screen: {fingerprint}"));
    said(&format!("{installation} asks to approve apps for: {}", ceiling.join(", ")));

    let expires = session["body"]["expires"].as_i64().unwrap_or(erislogin::now() + 600);
    let within = Duration::from_secs(expires.saturating_sub(erislogin::now()).max(1) as u64);
    match client.await_pairing(within, &erisdb_client::Cancel::new()).await? {
        erisdb_client::Pairing::Approved { token, granted } => {
            let joined = Joined {
                name: name.to_string(),
                eid: addr.id.to_string(),
                addrs: addr.ip_addrs().copied().collect(),
                token,
            };
            joined.save(paths)?;
            said(&format!("Joined. This device holds: {}", granted.join(", ")));
            Ok(joined)
        }
        erisdb_client::Pairing::Denied => bail!("the request to join was denied"),
        erisdb_client::Pairing::TimedOut => bail!("nobody approved the request to join in time"),
        other => bail!("joining ended without an answer: {other:?}"),
    }
}

/// The approval page's link, as the running daemon wrote it.
pub fn page_link(paths: &Paths) -> Result<String> {
    let path = page::link_path(&paths.runtime);
    let link = std::fs::read_to_string(&path).context("ErisAuth is not running; start `erisauth serve`")?;
    Ok(link.trim().to_string())
}

/// Listen for apps and for the person, on the socket and the approval page,
/// until the process ends.
pub async fn serve(paths: &Paths, notify: bool) -> Result<()> {
    let joined = Joined::load(paths)?;
    let db = match &joined {
        Some(joined) => Some(joined.dial(paths).await?),
        None => None,
    };
    let agent = agent::Agent::new(db, joined, notify);
    // The socket first: a daemon that cannot take it publishes no page link.
    let listener = socket::bind(&paths.socket())?;
    let (link, page) = page::listen(&paths.runtime, agent.clone()).await?;
    agent.set_page(link);
    tokio::try_join!(socket::serve(listener, agent), page)?;
    Ok(())
}

/// Open `link` in the person's browser.
pub fn open(link: &str) {
    if let Err(e) = std::process::Command::new("xdg-open").arg(link).status() {
        eprintln!("erisauth: could not open {link}: {e}");
    }
}

/// Create `dir` readable only by its owner.
pub fn private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    Ok(())
}
