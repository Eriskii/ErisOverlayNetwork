//! ErisOverlayNetwork on a device.
//!
//! [`Daemon::start`] loads the device's key, starts its mesh node, and answers on Iroh: the
//! mesh's own protocol, the ceremony a newcomer redeems a ticket over, and, while the device is
//! in no mesh yet, offers from devices that scanned its own ticket ([`adding`]). [`serve`] adds what a
//! computer needs around it: the [agent socket](socket) apps and the person's tools reach it
//! on, the loopback [approval page](page), and desktop [notifications](notify). On Android the
//! daemon runs in the app's process instead ([`mobile`]).
//!
//! What the person can do, the [commands](command) say; what apps and newcomers ask, and how
//! the person's answer is carried out, is in [`agent`].

pub mod adding;
pub mod agent;
pub mod changes;
pub mod command;
pub mod mobile;
pub mod notify;
pub mod page;
pub mod prompt;
pub mod socket;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use erisoverlaynetwork::ceremony::{self, PairService};
use erisoverlaynetwork::code;
use erisoverlaynetwork::join::{self, JoinService, Offers};
use erisoverlaynetwork::mesh::{self, Config, FileStore, Local, Node};
use erisoverlaynetwork::net::{self, Network};
use erisoverlaynetwork::pairing::Pairings;
use erisoverlaynetwork::{DeviceId, Note};
use iroh::protocol::Router;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use uuid::Uuid;

/// Where a device keeps its key and its mesh, and where it listens.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `$XDG_DATA_HOME/eon`: the key, the mesh, the notes this device issued.
    pub data: PathBuf,
    /// The session's runtime directory — `$XDG_RUNTIME_DIR`, or `$TMPDIR` on macOS — under which
    /// the socket and the page's link live.
    pub runtime: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let data = var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".local/share")))
            .context("neither XDG_DATA_HOME nor HOME is set")?
            .join("eon");
        let runtime = erisoverlaynetwork::sys::runtime_dir()
            .context("XDG_RUNTIME_DIR is not set; ErisOverlayNetwork listens in the session's runtime directory")?;
        Ok(Paths { data, runtime })
    }

    pub fn socket(&self) -> PathBuf {
        erisoverlaynetwork::agent::socket_in(&self.runtime)
    }
}

/// The device's key.
pub fn key_path(data: &Path) -> PathBuf {
    data.join("key")
}

/// The device list and directory, as the node keeps them.
pub fn mesh_path(data: &Path) -> PathBuf {
    data.join("mesh.json")
}

/// Every note this device issued.
pub fn notes_path(data: &Path) -> PathBuf {
    data.join("notes.json")
}

/// How long a ticket stays redeemable, and a request answerable.
pub const TICKET_SECS: i64 = 600;

/// What a ticket this device shows is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// A new device joins the mesh with it.
    Device,
    /// An app with no ErisOverlayNetwork beside it, such as one in a browser, asks for a note with it.
    App,
}

/// A join this device started, until it ends.
#[derive(Debug, Clone, Serialize)]
pub struct Joining {
    /// The code to compare with the approving device's screen.
    pub fingerprint: String,
    /// The approving device's name for itself.
    pub host: String,
    /// Why the join failed, once it has.
    pub error: Option<String>,
}

/// Something that changed in the mesh, said the way the person is told it.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub seq: u64,
    pub at: i64,
    pub text: String,
}

/// The most events the daemon remembers.
const EVENTS: usize = 50;

/// One device's ErisOverlayNetwork: its key, its node, and everything waiting for the person.
pub struct Daemon {
    pub(crate) network: Network,
    pub(crate) node: Node,
    pub(crate) pairings: Arc<Pairings>,
    /// What devices that scanned this one's own ticket offer it, while it is in no mesh.
    pub(crate) offers: Arc<Offers>,
    router: Router,
    data: PathBuf,
    notify: bool,
    /// The notes this device issued, oldest first.
    issued: Mutex<Vec<Note>>,
    /// What waits for the person, by request id.
    pub(crate) requests: Mutex<BTreeMap<Uuid, agent::Pending>>,
    /// The tickets this device opened and what each is for.
    pub(crate) sessions: Mutex<BTreeMap<Uuid, Purpose>>,
    /// The short codes standing in for this device's tickets, by session.
    pub(crate) codes: Mutex<HashMap<Uuid, code::Shown>>,
    /// The ticket this device shows to be added, while it is in no mesh.
    pub(crate) shown: Mutex<Option<adding::Shown>>,
    /// A device this one is adding by its ticket.
    pub(crate) inviting: Mutex<Option<adding::Inviting>>,
    joining: Mutex<Option<Joining>>,
    events: Mutex<VecDeque<Event>>,
    /// Whether each other device answered when last asked.
    pub(crate) reach: Mutex<HashMap<DeviceId, bool>>,
    changes: watch::Sender<u64>,
    /// The approval page's link, once it listens.
    page: OnceLock<String>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl Daemon {
    /// Load the key and the mesh kept in `data` and start answering on Iroh. With `notify`, the
    /// person hears of requests and changes through desktop notifications.
    pub async fn start(data: &Path, network: Network, notify: bool) -> Result<Arc<Self>> {
        private_dir(data)?;
        let key = erisoverlaynetwork::key::load_or_create(&key_path(data))?;
        let endpoint = net::endpoint(key.clone(), &network).await?;
        let node = Node::start(Config {
            endpoint: endpoint.clone(),
            network: network.clone(),
            local: Local::Device(key),
            store: Arc::new(FileStore::new(mesh_path(data))),
        })
        .await?;
        let pairings = Arc::new(Pairings::new());
        let offers = Arc::new(Offers::new(node.id()));
        let named = node.clone();
        let adopting = node.clone();
        let adopt: join::Adopt = Arc::new(move |snapshot| {
            let node = adopting.clone();
            Box::pin(async move { node.adopt(snapshot).await })
        });
        let router = Router::builder(endpoint)
            .accept(mesh::ALPN, node.protocol())
            .accept(ceremony::ALPN, PairService::new(pairings.clone(), move || device_name(&named).unwrap_or_default()))
            .accept(join::ALPN, JoinService::new(offers.clone(), adopt))
            .spawn();
        let issued = load_notes(&notes_path(data))?;
        let daemon = Arc::new(Daemon {
            network,
            node,
            pairings,
            offers,
            router,
            data: data.to_path_buf(),
            notify,
            issued: Mutex::new(issued),
            requests: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            codes: Mutex::new(HashMap::new()),
            shown: Mutex::new(None),
            inviting: Mutex::new(None),
            joining: Mutex::new(None),
            events: Mutex::new(VecDeque::new()),
            reach: Mutex::new(HashMap::new()),
            changes: watch::channel(0).0,
            page: OnceLock::new(),
            tasks: Mutex::new(Vec::new()),
        });
        let tasks = vec![
            tokio::spawn(daemon.clone().follow_sessions()),
            tokio::spawn(daemon.clone().follow_list()),
            tokio::spawn(daemon.clone().follow_offers()),
            tokio::spawn(daemon.clone().probe()),
        ];
        *daemon.tasks.lock().unwrap_or_else(|e| e.into_inner()) = tasks;
        Ok(daemon)
    }

    /// Stop answering and let the node go.
    pub async fn shutdown(&self) {
        for task in self.tasks.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            task.abort();
        }
        let codes: Vec<code::Shown> =
            self.codes.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, c)| c).collect();
        for code in codes {
            code.close().await;
        }
        let _ = self.router.shutdown().await;
        self.node.shutdown().await;
    }

    pub fn node(&self) -> &Node {
        &self.node
    }

    pub fn id(&self) -> DeviceId {
        self.node.id()
    }

    /// This device's name in the mesh, once it is in one.
    pub fn name(&self) -> Option<String> {
        device_name(&self.node)
    }

    /// Whether this device is a member of a mesh.
    pub fn is_member(&self) -> bool {
        self.node.with_list(|list| list.is_some_and(|list| list.is_member(&self.id())))
    }

    /// A receiver that changes whenever anything the person's screens show does.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    pub fn version(&self) -> u64 {
        *self.changes.borrow()
    }

    pub(crate) fn changed(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    pub fn set_page(&self, link: String) {
        let _ = self.page.set(link);
    }

    pub fn page(&self) -> Option<String> {
        self.page.get().cloned()
    }

    pub fn notifies(&self) -> bool {
        self.notify
    }

    pub fn joining(&self) -> Option<Joining> {
        self.joining.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_joining(&self, joining: Option<Joining>) {
        *self.joining.lock().unwrap_or_else(|e| e.into_inner()) = joining;
        self.changed();
    }

    /// The notes this device issued, oldest first.
    pub fn issued(&self) -> Vec<Note> {
        self.issued.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Keep `note` among those this device issued.
    pub(crate) fn record(&self, note: Note) -> Result<()> {
        let mut issued = self.issued.lock().unwrap_or_else(|e| e.into_inner());
        issued.push(note);
        let bytes = serde_json::to_vec_pretty(&*issued)?;
        mesh::write_private(&notes_path(&self.data), &bytes)?;
        Ok(())
    }

    /// The latest events, oldest first.
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }

    /// Tell the person something changed.
    pub(crate) fn tell(&self, text: String) {
        let event = {
            let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
            let seq = events.back().map(|e| e.seq + 1).unwrap_or(1);
            let event = Event { seq, at: erisoverlaynetwork::now(), text };
            events.push_back(event.clone());
            while events.len() > EVENTS {
                events.pop_front();
            }
            event
        };
        if self.notify {
            notify::event(&event, self.page());
        }
        self.changed();
    }
}

/// A ticket's QR code: `{svg, qr: {side, modules}}`, drawn as an SVG for the page and laid out for
/// the Android app to draw: `side` modules a side, `modules` a `1` for each dark one, row by row.
pub(crate) fn qr(ticket: &str) -> erisoverlaynetwork::Result<Value> {
    let svg = erisoverlaynetwork::qr::svg(ticket)?;
    let (side, raster) = erisoverlaynetwork::qr::raster(ticket, 1)?;
    let modules: String = raster.iter().map(|&p| if p == 0 { '1' } else { '0' }).collect();
    Ok(json!({"svg": svg, "qr": {"side": side, "modules": modules}}))
}

/// What this device is called in its mesh, once it is in one.
fn device_name(node: &Node) -> Option<String> {
    node.with_list(|list| list.and_then(|list| list.member(&node.id()).map(|m| m.name.clone())))
}

fn load_notes(path: &Path) -> Result<Vec<Note>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("{} is damaged", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Run ErisOverlayNetwork on a computer until the process is told to stop: the daemon, the agent socket,
/// the approval page and, with `notify`, desktop notifications.
pub async fn serve(paths: &Paths, network: Network, notify: bool) -> Result<()> {
    // The socket first: a daemon that cannot take it publishes no page link.
    let listener = socket::bind(&paths.socket())?;
    let daemon = Daemon::start(&paths.data, network, notify).await?;
    let (link, page) = page::listen(&paths.runtime, daemon.clone()).await?;
    daemon.set_page(link);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let outcome = tokio::select! {
        served = socket::serve(listener, daemon.clone()) => served,
        served = page => served,
        _ = tokio::signal::ctrl_c() => Ok(()),
        _ = terminate.recv() => Ok(()),
        _ = daemon.node().closed() => Err(anyhow::anyhow!(
            "this device's iroh endpoint closed by itself (its sockets failed, as after a network change it \
             could not rebind to); start eon again"
        )),
    };
    let _ = std::fs::remove_file(paths.socket());
    let _ = std::fs::remove_file(page::link_path(&paths.runtime));
    daemon.shutdown().await;
    outcome
}

/// The approval page's link, as the running daemon wrote it.
pub fn page_link(paths: &Paths) -> Result<String> {
    let path = page::link_path(&paths.runtime);
    let link = std::fs::read_to_string(&path).context("ErisOverlayNetwork is not running; start `eon serve`")?;
    Ok(link.trim().to_string())
}

/// Open `link` in the person's browser.
pub fn open(link: &str) {
    if let Err(e) = erisoverlaynetwork::sys::open(link) {
        eprintln!("eon: could not open {link}: {e}");
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
