//! The node every participant runs: its copy of the mesh, kept current.
//!
//! A [`Node`] holds the [device list](crate::devices) and the [directory](crate::directory)
//! and speaks [`ALPN`] with every other participant:
//!
//! - **sync**: both sides say who they are and where, compare the list's heads, and send each
//!   other whatever entries the other lacks, along with everyone they know of. Every
//!   connection starts with one, so two participants that talk always agree;
//! - **push**: what a participant learns it sends on to everyone it knows, at once, so a
//!   removal reaches every reachable participant without waiting for anyone to ask.
//!
//! A node also syncs with everyone when it starts, whenever its addresses change, and every
//! [`ANNOUNCE`] besides. A device's node — ErisAuth's — signs: it writes entries and issues
//! notes. An app's node takes part under its note.
//!
//! A service checks who is calling with [`Node::authorize`], and watches [`Node::subscribe`]
//! to drop whatever stops counting the moment it does.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iroh::address_lookup::MemoryLookup;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointId, SecretKey};
use n0_future::boxed::BoxFuture;
use n0_future::task::AbortOnDropHandle;
use serde::{Deserialize, Serialize};

use crate::devices::{DeviceId, DeviceList, EntryId, Member, MeshId, Op};
use crate::directory::{Presence, Role, Snapshot};
use crate::error::{Error, Result};
use crate::net::Network;
use crate::note::{Note, NoteBody};
use crate::signed::Signed;

/// The mesh's ALPN.
pub const ALPN: &[u8] = b"eris-mesh/0";

/// How often a node syncs with everyone, besides when it starts and when it moves.
pub const ANNOUNCE: Duration = Duration::from_secs(300);

/// How recently a node must have synced with a participant before it dials it for a service.
pub const FRESH: Duration = Duration::from_secs(120);

/// Who this node is in the mesh.
#[derive(Clone)]
pub enum Local {
    /// A device: ErisAuth, holding the device's key.
    Device(SecretKey),
    /// An app, under its note.
    App(Note),
}

impl std::fmt::Debug for Local {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Local::Device(key) => write!(f, "Device({})", key.public().fmt_short()),
            Local::App(note) => write!(f, "App({})", note.body.name),
        }
    }
}

/// What a node keeps between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub entries: Vec<Signed>,
    #[serde(default)]
    pub directory: Vec<Presence>,
}

/// Where a node keeps what it knows.
pub trait Store: Send + Sync + 'static {
    fn load(&self) -> BoxFuture<Result<Saved>>;
    fn save(&self, saved: Saved) -> BoxFuture<Result<()>>;
}

/// A store in one JSON file, rewritten atomically, owner-only.
#[derive(Debug, Clone)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileStore { path: path.into() }
    }
}

impl Store for FileStore {
    fn load(&self) -> BoxFuture<Result<Saved>> {
        let path = self.path.clone();
        Box::pin(async move {
            match tokio::fs::read(&path).await {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Internal(format!("{} is damaged: {e}", path.display()))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Saved::default()),
                Err(e) => Err(e.into()),
            }
        })
    }

    fn save(&self, saved: Saved) -> BoxFuture<Result<()>> {
        let path = self.path.clone();
        Box::pin(async move {
            let bytes = serde_json::to_vec_pretty(&saved).map_err(|e| Error::Internal(e.to_string()))?;
            tokio::task::spawn_blocking(move || write_private(&path, &bytes))
                .await
                .map_err(|e| Error::Internal(e.to_string()))?
        })
    }
}

/// A store that forgets everything when the node stops.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<Saved>);

impl Store for MemoryStore {
    fn load(&self) -> BoxFuture<Result<Saved>> {
        let saved = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Box::pin(async move { Ok(saved) })
    }

    fn save(&self, saved: Saved) -> BoxFuture<Result<()>> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = saved;
        Box::pin(async { Ok(()) })
    }
}

/// Write `bytes` to `path` atomically, readable by its owner only.
pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// What a node needs to start.
pub struct Config {
    pub endpoint: Endpoint,
    pub network: Network,
    pub local: Local,
    pub store: Arc<dyn Store>,
}

/// A request over [`ALPN`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Sync {
        mesh: MeshId,
        me: Presence,
        heads: Vec<EntryId>,
        /// Entries the asker thinks the other may lack: all of them, the first time.
        #[serde(default)]
        entries: Vec<Signed>,
        #[serde(default)]
        directory: Vec<Presence>,
    },
    Push {
        mesh: MeshId,
        #[serde(default)]
        entries: Vec<Signed>,
        #[serde(default)]
        directory: Vec<Presence>,
    },
}

/// The answer to a sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Synced {
    pub me: Presence,
    pub heads: Vec<EntryId>,
    /// Every entry, when the asker's heads were not this side's.
    #[serde(default)]
    pub entries: Vec<Signed>,
    #[serde(default)]
    pub directory: Vec<Presence>,
}

/// A caller a service let in.
#[derive(Debug, Clone)]
pub struct Caller {
    pub app: EndpointId,
    pub device: DeviceId,
    /// The device's name, as the list has it.
    pub device_name: String,
    pub note: Note,
}

#[derive(Default)]
struct State {
    list: Option<DeviceList>,
    directory: BTreeMap<EndpointId, Presence>,
}

struct Inner {
    ep: Endpoint,
    network: Network,
    local: Local,
    store: Arc<dyn Store>,
    state: Mutex<State>,
    changes: tokio::sync::watch::Sender<u64>,
    lookup: MemoryLookup,
    conns: Mutex<HashMap<EndpointId, Connection>>,
    synced: Mutex<HashMap<EndpointId, n0_future::time::Instant>>,
    tasks: Mutex<Vec<AbortOnDropHandle<()>>>,
}

/// One participant's node. Clones share it.
#[derive(Clone)]
pub struct Node {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Node({:?}, {})", self.inner.local, self.inner.ep.id().fmt_short())
    }
}

impl Node {
    /// Load what the store holds and start keeping it current.
    pub async fn start(config: Config) -> Result<Node> {
        let Config { endpoint, network, local, store } = config;
        if let Local::App(note) = &local
            && note.app() != endpoint.id()
        {
            return Err(Error::BadRequest("an app's node runs under the key its note names".into()));
        }
        if let Local::Device(key) = &local
            && key.public() != endpoint.id()
        {
            return Err(Error::BadRequest("a device's node runs under the device's key".into()));
        }
        let saved = store.load().await?;
        let list = if saved.entries.is_empty() { None } else { Some(DeviceList::from_entries(saved.entries)?) };
        let lookup = MemoryLookup::new();
        endpoint.address_lookup().map_err(|e| Error::Internal(e.to_string()))?.add(lookup.clone());
        let node = Node {
            inner: Arc::new(Inner {
                ep: endpoint,
                network,
                local,
                store,
                state: Mutex::new(State { list, directory: BTreeMap::new() }),
                changes: tokio::sync::watch::channel(0).0,
                lookup,
                conns: Mutex::new(HashMap::new()),
                synced: Mutex::new(HashMap::new()),
                tasks: Mutex::new(Vec::new()),
            }),
        };
        node.learn(saved.directory);
        node.prune();
        let announcer = node.clone();
        let task = n0_future::task::spawn(async move { announcer.announce().await });
        node.inner.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(AbortOnDropHandle::new(task));
        Ok(node)
    }

    /// The protocol handler to register on the participant's router under [`ALPN`].
    pub fn protocol(&self) -> MeshProtocol {
        MeshProtocol { node: self.clone() }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.inner.ep
    }

    pub fn id(&self) -> EndpointId {
        self.inner.ep.id()
    }

    pub fn local(&self) -> &Local {
        &self.inner.local
    }

    /// This node's mesh, once it has a list.
    pub fn mesh(&self) -> Option<MeshId> {
        self.read(|s| s.list.as_ref().map(DeviceList::mesh))
    }

    /// A copy of the list as this node has it.
    pub fn list(&self) -> Option<DeviceList> {
        self.read(|s| s.list.clone())
    }

    /// The mesh's member devices.
    pub fn members(&self) -> Vec<(DeviceId, Member)> {
        self.read(|s| {
            s.list.as_ref().map(|l| l.state().members.iter().map(|(d, m)| (*d, m.clone())).collect()).unwrap_or_default()
        })
    }

    /// Everyone this node knows of and where, itself excluded.
    pub fn directory(&self) -> Vec<Presence> {
        self.read(|s| s.directory.values().cloned().collect())
    }

    /// The participants serving `service`, other than this one.
    pub fn providers(&self, service: &str) -> Vec<Presence> {
        self.read(|s| {
            s.directory
                .values()
                .filter(|p| p.note().is_some_and(|n| n.body.serves && n.body.service == service))
                .cloned()
                .collect()
        })
    }

    /// The presence of the app pinned to provide `service`, when it still counts.
    pub fn pinned(&self, service: &str) -> Option<Presence> {
        let me = self.id();
        self.read(|s| {
            let list = s.list.as_ref()?;
            let pin = *list.pinned(service)?;
            if pin.app == me {
                return None;
            }
            s.directory.get(&pin.app).filter(|p| p.note().is_some_and(|n| n.id == pin.note)).cloned()
        })
    }

    /// Everything a newcomer needs, this node's own presence included.
    pub async fn snapshot(&self) -> Option<Snapshot> {
        let me = self.presence().await;
        self.read(|s| {
            let list = s.list.as_ref()?;
            let mut directory: Vec<Presence> = s.directory.values().cloned().collect();
            directory.push(me);
            Some(Snapshot { mesh: list.mesh(), entries: list.entries(), directory })
        })
    }

    /// A receiver that changes whenever the list or the directory does.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.changes.subscribe()
    }

    /// Whether `caller` may use `service` here, and as whom; with `required`, whether its note
    /// also grants that.
    pub fn authorize(&self, caller: EndpointId, service: &str, required: Option<&str>) -> Result<Caller> {
        self.read(|s| {
            let list = s.list.as_ref().ok_or(Error::Unauthorized)?;
            let presence = s.directory.get(&caller).ok_or(Error::Forbidden { permission: "a known participant".into() })?;
            let note = presence.note().ok_or(Error::Forbidden { permission: format!("an app approved for {service}") })?;
            note.check(list)?;
            if note.app() != caller || note.body.service != service {
                return Err(Error::Forbidden { permission: format!("an app approved for {service}") });
            }
            if let Some(required) = required
                && !note.grants(service, required)
            {
                return Err(Error::Forbidden { permission: required.to_string() });
            }
            let device_name = list.member(&note.device()).map(|m| m.name.clone()).unwrap_or_default();
            Ok(Caller { app: caller, device: note.device(), device_name, note: note.clone() })
        })
    }

    /// Start a mesh with this device as its first member.
    pub async fn create(&self, name: &str) -> Result<MeshId> {
        let Local::Device(key) = &self.inner.local else {
            return Err(Error::Forbidden { permission: "a device".into() });
        };
        let (list, _) = DeviceList::create(key, name)?;
        let mesh = list.mesh();
        self.write(|s| {
            if s.list.is_some() {
                return Err(Error::Conflict("this device is in a mesh already".into()));
            }
            s.list = Some(list);
            Ok(())
        })?;
        self.save().await?;
        self.changed();
        Ok(mesh)
    }

    /// Take in a snapshot: a newcomer's first copy of the mesh, or more of the one it has.
    pub async fn adopt(&self, snapshot: Snapshot) -> Result<()> {
        let incoming = snapshot.list()?;
        self.write(|s| match &mut s.list {
            None => {
                s.list = Some(incoming);
                Ok(())
            }
            Some(list) if list.mesh() == snapshot.mesh => {
                list.merge(snapshot.entries.iter().cloned());
                Ok(())
            }
            Some(_) => Err(Error::Conflict("this node is in another mesh".into())),
        })?;
        self.learn(snapshot.directory);
        self.prune();
        self.save().await?;
        self.changed();
        // A newcomer tells everyone it now knows of who it is and where.
        let node = self.clone();
        n0_future::task::spawn(async move { node.sync_all().await });
        Ok(())
    }

    /// Write an entry as this device and send it to everyone.
    pub async fn append(&self, op: Op) -> Result<Signed> {
        let Local::Device(key) = &self.inner.local else {
            return Err(Error::Forbidden { permission: "a device".into() });
        };
        let signed = self.write(|s| s.list.as_mut().ok_or(Error::Conflict("not in a mesh".into()))?.append(key, op))?;
        self.prune();
        self.save().await?;
        self.changed();
        self.spread(vec![signed.clone()], None);
        Ok(signed)
    }

    /// Approve an app, as this device.
    pub fn issue(
        &self,
        app: EndpointId,
        name: &str,
        service: &str,
        grants: Vec<String>,
        serves: bool,
        program: Option<String>,
    ) -> Result<Note> {
        let Local::Device(key) = &self.inner.local else {
            return Err(Error::Forbidden { permission: "a device".into() });
        };
        let mesh = self.mesh().ok_or(Error::Conflict("not in a mesh".into()))?;
        Note::issue(
            key,
            NoteBody {
                mesh,
                device: key.public(),
                app,
                name: crate::check_name(name)?,
                service: service.to_string(),
                grants,
                serves,
                program,
                at: crate::now(),
            },
        )
    }

    /// This node's own presence, now.
    pub async fn presence(&self) -> Presence {
        let addr = crate::net::advertised(&self.inner.ep, &self.inner.network).await;
        Presence {
            id: self.id(),
            role: match &self.inner.local {
                Local::Device(_) => Role::Device,
                Local::App(note) => Role::App { note: note.clone() },
            },
            addrs: addr.ip_addrs().copied().take(crate::directory::MAX_ADDRS).collect(),
            relay: addr.relay_urls().next().map(|r| r.to_string()),
            at: crate::now(),
        }
    }

    /// Dial `id` for `alpn`, having synced with it lately enough that it knows who is calling.
    pub async fn connect(&self, id: EndpointId, alpn: &[u8]) -> Result<Connection> {
        let fresh = self
            .inner
            .synced
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .is_some_and(|t| t.elapsed() < FRESH);
        if !fresh {
            self.sync_with(id).await?;
        }
        n0_future::time::timeout(crate::net::CONNECT_TIMEOUT, self.inner.ep.connect(self.addr_of(id), alpn))
            .await
            .map_err(|_| Error::Unreachable("the participant did not answer in time".into()))?
            .map_err(|e| Error::Unreachable(format!("could not reach the participant: {e}")))
    }

    /// Sync with one participant.
    pub async fn sync_with(&self, id: EndpointId) -> Result<()> {
        let Some((mesh, heads)) = self.read(|s| s.list.as_ref().map(|l| (l.mesh(), l.heads().to_vec()))) else {
            return Err(Error::Conflict("not in a mesh".into()));
        };
        let first = !self.inner.synced.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id);
        let entries = if first { self.read(|s| s.list.as_ref().map(DeviceList::entries).unwrap_or_default()) } else { Vec::new() };
        let me = self.presence().await;
        let request = Request::Sync { mesh, me, heads: heads.clone(), entries, directory: self.directory() };
        let synced: Synced = self.ask(id, &request).await?;
        if synced.me.id != id {
            return Err(Error::Unreachable("the participant answered as someone else".into()));
        }
        let fresh = self.take(synced.entries, vec![synced.me].into_iter().chain(synced.directory).collect(), Some(id)).await?;
        let mine = self.read(|s| s.list.as_ref().map(|l| l.heads().to_vec()).unwrap_or_default());
        if mine != synced.heads {
            let entries = self.read(|s| s.list.as_ref().map(DeviceList::entries).unwrap_or_default());
            let push = Request::Push { mesh, entries, directory: Vec::new() };
            let _: serde_json::Value = self.ask(id, &push).await?;
        }
        self.inner.synced.lock().unwrap_or_else(|e| e.into_inner()).insert(id, n0_future::time::Instant::now());
        tracing::debug!(peer = %id.fmt_short(), fresh = fresh.len(), "synced");
        Ok(())
    }

    /// Sync with everyone this node knows of, at once.
    pub async fn sync_all(&self) {
        let everyone = self.everyone();
        n0_future::join_all(everyone.into_iter().map(|id| {
            let node = self.clone();
            async move {
                if let Err(e) = node.sync_with(id).await {
                    tracing::debug!(peer = %id.fmt_short(), error = %e, "sync");
                }
            }
        }))
        .await;
    }

    pub async fn shutdown(&self) {
        self.inner.tasks.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.inner.conns.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    // What follows is the node's own business.

    fn read<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&self.inner.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn write<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.inner.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn changed(&self) {
        self.inner.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    async fn save(&self) -> Result<()> {
        let saved = self.read(|s| Saved {
            entries: s.list.as_ref().map(DeviceList::entries).unwrap_or_default(),
            directory: s.directory.values().cloned().collect(),
        });
        self.inner.store.save(saved).await
    }

    /// Every participant this node knows of: the member devices and the apps in its directory.
    fn everyone(&self) -> Vec<EndpointId> {
        let me = self.id();
        self.read(|s| {
            let mut ids: HashSet<EndpointId> = s.directory.keys().copied().collect();
            if let Some(list) = &s.list {
                ids.extend(list.state().members.keys().copied());
            }
            ids.remove(&me);
            ids.into_iter().collect()
        })
    }

    fn addr_of(&self, id: EndpointId) -> iroh::EndpointAddr {
        self.read(|s| s.directory.get(&id).map(Presence::endpoint_addr)).unwrap_or_else(|| id.into())
    }

    async fn connection(&self, id: EndpointId) -> Result<Connection> {
        if let Some(conn) = self.inner.conns.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
            && conn.close_reason().is_none()
        {
            return Ok(conn);
        }
        let conn = n0_future::time::timeout(crate::net::CONNECT_TIMEOUT, self.inner.ep.connect(self.addr_of(id), ALPN))
            .await
            .map_err(|_| Error::Unreachable("the participant did not answer in time".into()))?
            .map_err(|e| Error::Unreachable(format!("could not reach the participant: {e}")))?;
        self.inner.conns.lock().unwrap_or_else(|e| e.into_inner()).insert(id, conn.clone());
        Ok(conn)
    }

    async fn ask<A: serde::de::DeserializeOwned>(&self, id: EndpointId, request: &Request) -> Result<A> {
        let conn = self.connection(id).await?;
        match crate::wire::ask(&conn, request).await {
            Err(Error::Unreachable(_)) => {
                self.inner.conns.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                let conn = self.connection(id).await?;
                crate::wire::ask(&conn, request).await
            }
            other => other,
        }
    }

    /// Take in entries and presences from `from`; returns the entries new to this node, which
    /// it has sent on to everyone else.
    async fn take(&self, entries: Vec<Signed>, presences: Vec<Presence>, from: Option<EndpointId>) -> Result<Vec<EntryId>> {
        let fresh = self.write(|s| s.list.as_mut().map(|l| l.merge(entries)).unwrap_or_default());
        let learned = self.learn(presences);
        if !fresh.is_empty() {
            self.prune();
        }
        if !fresh.is_empty() || learned {
            self.save().await?;
            self.changed();
        }
        if !fresh.is_empty() {
            let spread: Vec<Signed> = self.read(|s| {
                let list = s.list.as_ref().expect("entries merged into a list");
                list.entries().into_iter().filter(|e| fresh.iter().any(|id| list_id(e) == *id)).collect()
            });
            self.spread(spread, from);
        }
        Ok(fresh)
    }

    /// Record the presences that check out against the list, keeping the newest of each.
    fn learn(&self, presences: Vec<Presence>) -> bool {
        let me = self.id();
        let mut learned = false;
        self.write(|s| {
            let Some(list) = &s.list else { return };
            for presence in presences {
                if presence.id == me || presence.check(list).is_err() {
                    continue;
                }
                if s.directory.get(&presence.id).is_some_and(|known| known.at >= presence.at) {
                    continue;
                }
                self.inner.lookup.set_endpoint_info(presence.endpoint_addr());
                s.directory.insert(presence.id, presence);
                learned = true;
            }
        });
        learned
    }

    /// Forget whoever stopped counting, and hang up on them.
    fn prune(&self) {
        let gone: Vec<EndpointId> = self.write(|s| {
            let Some(list) = &s.list else { return Vec::new() };
            let gone: Vec<EndpointId> =
                s.directory.iter().filter(|(_, p)| p.check(list).is_err()).map(|(id, _)| *id).collect();
            for id in &gone {
                s.directory.remove(id);
            }
            let removed: Vec<EndpointId> = list.state().removed.iter().copied().collect();
            gone.into_iter().chain(removed).collect()
        });
        let mut conns = self.inner.conns.lock().unwrap_or_else(|e| e.into_inner());
        for id in gone {
            if let Some(conn) = conns.remove(&id) {
                conn.close(0u32.into(), b"left the mesh");
            }
            self.inner.lookup.remove_endpoint_info(id);
        }
    }

    /// Send entries to everyone but `from`, without waiting.
    fn spread(&self, entries: Vec<Signed>, from: Option<EndpointId>) {
        let Some(mesh) = self.mesh() else { return };
        let targets: Vec<EndpointId> = self.everyone().into_iter().filter(|id| Some(*id) != from).collect();
        if targets.is_empty() || entries.is_empty() {
            return;
        }
        let node = self.clone();
        n0_future::task::spawn(async move {
            let request = Request::Push { mesh, entries, directory: Vec::new() };
            n0_future::join_all(targets.into_iter().map(|id| {
                let node = node.clone();
                let request = request.clone();
                async move {
                    if let Err(e) = node.ask::<serde_json::Value>(id, &request).await {
                        tracing::debug!(peer = %id.fmt_short(), error = %e, "push");
                    }
                }
            }))
            .await;
        });
    }

    /// Sync with everyone now, every [`ANNOUNCE`], and whenever this endpoint's address changes.
    async fn announce(&self) {
        use iroh::Watcher;
        let mut addrs = self.inner.ep.watch_addr();
        let mut tick = n0_future::time::interval(ANNOUNCE);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                moved = addrs.updated() => if moved.is_err() { return },
            }
            if self.mesh().is_some() {
                self.sync_all().await;
            }
        }
    }

    async fn handle(&self, remote: EndpointId, request: Request) -> Result<serde_json::Value> {
        let Some(mesh) = self.mesh() else {
            return Err(Error::Conflict("this participant is in no mesh yet".into()));
        };
        match request {
            Request::Sync { mesh: theirs, me, heads, entries, directory } => {
                if theirs != mesh {
                    return Err(Error::Forbidden { permission: "the same mesh".into() });
                }
                // Entries check themselves, so they are taken before the caller is.
                self.take(entries, Vec::new(), Some(remote)).await?;
                if me.id != remote {
                    return Err(Error::Unauthorized);
                }
                self.read(|s| me.check(s.list.as_ref().expect("in a mesh")))?;
                self.take(Vec::new(), std::iter::once(me).chain(directory).collect(), Some(remote)).await?;
                let (mine, all) = self.read(|s| {
                    let list = s.list.as_ref().expect("in a mesh");
                    (list.heads().to_vec(), list.entries())
                });
                let reply = Synced {
                    me: self.presence().await,
                    entries: if mine != heads { all } else { Vec::new() },
                    heads: mine,
                    directory: self.directory().into_iter().filter(|p| p.id != remote).collect(),
                };
                self.inner.synced.lock().unwrap_or_else(|e| e.into_inner()).insert(remote, n0_future::time::Instant::now());
                serde_json::to_value(reply).map_err(|e| Error::Internal(e.to_string()))
            }
            Request::Push { mesh: theirs, entries, directory } => {
                if theirs != mesh {
                    return Err(Error::Forbidden { permission: "the same mesh".into() });
                }
                let known = self.read(|s| {
                    s.directory.contains_key(&remote) || s.list.as_ref().is_some_and(|l| l.is_member(&remote))
                });
                if !known {
                    return Err(Error::Forbidden { permission: "a known participant".into() });
                }
                self.take(entries, directory, Some(remote)).await?;
                Ok(serde_json::Value::Null)
            }
        }
    }
}

fn list_id(signed: &Signed) -> EntryId {
    EntryId(signed.digest(crate::devices::ENTRY_DOMAIN))
}

/// A node's side of [`ALPN`], for a participant's router.
#[derive(Clone)]
pub struct MeshProtocol {
    node: Node,
}

impl std::fmt::Debug for MeshProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MeshProtocol({:?})", self.node)
    }
}

impl MeshProtocol {
    /// Serve one connection: what a host without an iroh router calls for each it accepts.
    pub async fn serve(&self, conn: Connection) {
        let remote = conn.remote_id();
        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
            let node = self.node.clone();
            n0_future::task::spawn(async move {
                let answer = match crate::wire::request::<Request>(&mut recv).await {
                    Ok(request) => node.handle(remote, request).await,
                    Err(e) => Err(e),
                };
                let _ = crate::wire::reply(&mut send, answer).await;
            });
        }
    }
}

impl ProtocolHandler for MeshProtocol {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
        self.serve(conn).await;
        Ok(())
    }
}
