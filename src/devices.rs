//! The device list: who is in a mesh, as signed entries every participant checks for itself.
//!
//! The first entry, the genesis, names the device that made the mesh; its id is the mesh's
//! id. Every later entry is signed by a device and names the entries its author had seen, its
//! parents, so the entries form a graph that every copy orders the same way. Entries say:
//!
//! - **add** a device, under a name;
//! - **remove** devices, for good: a removed key never counts again;
//! - **name** a device anew;
//! - **revoke** an app's [note](crate::note).
//!
//! Any member device may write any of them. What a set of entries means is decided by these
//! rules alone, so every participant holding the same entries agrees on the same members:
//!
//! 1. An entry counts only if its author was a member when it wrote it, as far as its own
//!    parents show.
//! 2. A removal counts whenever its author was a member when it wrote it. Two devices that
//!    remove each other at once are both removed.
//! 3. Once a device is removed, the other things it wrote count only if the removal's author
//!    had seen them. A stolen phone cannot add a computer behind the back of the device that
//!    removes it: the computer's addition was not among what the remover had seen.
//!
//! The list holds no secrets. What it guards is which keys count.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use iroh::SecretKey;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::note::NoteId;
use crate::signed::{Signed, digest_id};

/// A device: the public key ErisAuth holds on it, which is also its Iroh endpoint id.
pub type DeviceId = iroh::PublicKey;

/// Domain tag for list entries.
pub const ENTRY_DOMAIN: &str = "eris-mesh/entry/0";

/// The most entries one list holds.
pub const MAX_ENTRIES: usize = 10_000;

/// The most entries one list holds while their parents have not arrived.
pub const MAX_WAITING: usize = 1_000;

/// The most devices one removal names.
pub const MAX_REMOVED: usize = 64;

digest_id!(
    /// A mesh, named by the id of its genesis entry.
    MeshId
);
digest_id!(
    /// One entry in a device list.
    EntryId
);

/// What an entry says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// The mesh begins; its author is its first device.
    Genesis {
        name: String,
    },
    Add {
        device: DeviceId,
        name: String,
    },
    Remove {
        devices: Vec<DeviceId>,
    },
    Name {
        device: DeviceId,
        name: String,
    },
    Revoke {
        note: NoteId,
    },
}

/// What an entry's author signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    /// The mesh this entry belongs to; absent only on a genesis, whose id the mesh's is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshId>,
    /// The entries its author had seen last, sorted.
    pub parents: Vec<EntryId>,
    /// Unix seconds, as its author's clock read. Shown to people; ordering comes from parents.
    pub at: i64,
    #[serde(flatten)]
    pub op: Op,
}

/// A checked entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: EntryId,
    pub author: DeviceId,
    pub body: Body,
    pub signed: Signed,
}

impl Entry {
    /// Check a signed entry and read it.
    pub fn open(signed: Signed) -> Result<Self> {
        let body: Body = signed.open(ENTRY_DOMAIN)?;
        let id = EntryId(signed.digest(ENTRY_DOMAIN));
        let entry = Entry { id, author: signed.by, body, signed };
        entry.check_shape()?;
        Ok(entry)
    }

    fn check_shape(&self) -> Result<()> {
        let body = &self.body;
        match &body.op {
            Op::Genesis { name } => {
                if body.mesh.is_some() || !body.parents.is_empty() {
                    return Err(Error::BadRequest("a genesis has no mesh and no parents".into()));
                }
                crate::check_name(name)?;
            }
            op => {
                if body.mesh.is_none() || body.parents.is_empty() {
                    return Err(Error::BadRequest("an entry names its mesh and its parents".into()));
                }
                match op {
                    Op::Add { name, .. } | Op::Name { name, .. } => {
                        crate::check_name(name)?;
                    }
                    Op::Remove { devices } => {
                        if devices.is_empty() || devices.len() > MAX_REMOVED {
                            return Err(Error::BadRequest(format!("a removal names 1 to {MAX_REMOVED} devices")));
                        }
                    }
                    Op::Revoke { .. } | Op::Genesis { .. } => {}
                }
            }
        }
        if body.parents.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::BadRequest("an entry's parents are sorted and distinct".into()));
        }
        Ok(())
    }
}

/// A device in the list, as the list stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    /// The device that added it; none for the device that made the mesh.
    pub added_by: Option<DeviceId>,
    /// When it was added, by the adder's clock.
    pub added_at: i64,
}

/// What the entries mean together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub members: BTreeMap<DeviceId, Member>,
    /// Every device ever removed. None of them counts again.
    pub removed: BTreeSet<DeviceId>,
    pub revoked: BTreeSet<NoteId>,
    /// The entries nothing has named as a parent yet, sorted.
    pub heads: Vec<EntryId>,
}

/// One mesh's device list.
#[derive(Debug, Clone)]
pub struct DeviceList {
    mesh: MeshId,
    founder: DeviceId,
    entries: HashMap<EntryId, Entry>,
    /// Entries whose parents have not arrived yet. They count for nothing until they do.
    waiting: HashMap<EntryId, Entry>,
    /// Every entry, parents before children, ties broken by id.
    order: Vec<EntryId>,
    state: State,
}

impl DeviceList {
    /// Make a new mesh with `key` as its first device.
    pub fn create(key: &SecretKey, name: &str) -> Result<(Self, Signed)> {
        let body = Body {
            mesh: None,
            parents: Vec::new(),
            at: crate::now(),
            op: Op::Genesis { name: crate::check_name(name)? },
        };
        let signed = Signed::sign(key, ENTRY_DOMAIN, &body)?;
        let list = Self::from_entries([signed.clone()])?;
        Ok((list, signed))
    }

    /// A list from its entries, in any order. Exactly one must be a genesis.
    pub fn from_entries(entries: impl IntoIterator<Item = Signed>) -> Result<Self> {
        let opened: Vec<Entry> = entries.into_iter().map(Entry::open).collect::<Result<_>>()?;
        let mut geneses = opened.iter().filter(|e| matches!(e.body.op, Op::Genesis { .. }));
        let genesis = geneses.next().ok_or_else(|| Error::BadRequest("a device list starts with a genesis".into()))?;
        if geneses.next().is_some() {
            return Err(Error::BadRequest("a device list has one genesis".into()));
        }
        let mut list = DeviceList {
            mesh: MeshId(genesis.id.0),
            founder: genesis.author,
            entries: HashMap::new(),
            waiting: HashMap::new(),
            order: Vec::new(),
            state: State::default(),
        };
        let genesis = genesis.clone();
        list.entries.insert(genesis.id, genesis);
        list.insert(opened.into_iter().filter(|e| !matches!(e.body.op, Op::Genesis { .. })));
        Ok(list)
    }

    /// Add whatever of `entries` this list lacks and can place; returns the ids it took.
    ///
    /// An entry that does not verify, belongs to another mesh, or could never count is
    /// dropped; everyone holding the same entries drops the same ones. One whose parents have
    /// not arrived waits for them, counting for nothing until they do; see [`DeviceList::waiting`].
    pub fn merge(&mut self, entries: impl IntoIterator<Item = Signed>) -> Vec<EntryId> {
        let opened: Vec<Entry> = entries
            .into_iter()
            .filter_map(|signed| match Entry::open(signed) {
                Ok(entry) => Some(entry),
                Err(e) => {
                    tracing::debug!(error = %e, "dropping an entry that does not verify");
                    None
                }
            })
            .collect();
        self.insert(opened.into_iter())
    }

    fn insert(&mut self, entries: impl Iterator<Item = Entry>) -> Vec<EntryId> {
        let mut waiting: Vec<Entry> = entries
            .filter(|e| !self.entries.contains_key(&e.id) && e.body.mesh == Some(self.mesh))
            .chain(std::mem::take(&mut self.waiting).into_values())
            .collect();
        let mut taken = Vec::new();
        loop {
            let before = waiting.len();
            waiting.retain(|entry| {
                if self.entries.len() >= MAX_ENTRIES || self.entries.contains_key(&entry.id) {
                    return false;
                }
                if entry.body.parents.iter().all(|p| self.entries.contains_key(p)) {
                    self.entries.insert(entry.id, entry.clone());
                    taken.push(entry.id);
                    false
                } else {
                    true
                }
            });
            if waiting.is_empty() || waiting.len() == before {
                break;
            }
        }
        for entry in waiting.into_iter().take(MAX_WAITING) {
            self.waiting.insert(entry.id, entry);
        }
        if !taken.is_empty() || self.order.is_empty() {
            let dropped = self.recompute();
            taken.retain(|id| !dropped.contains(id));
        }
        taken
    }

    /// Write an entry as `key`, which must be a member's, on top of everything this list holds.
    pub fn append(&mut self, key: &SecretKey, op: Op) -> Result<Signed> {
        let author = key.public();
        if !self.is_member(&author) {
            return Err(Error::Forbidden { permission: "a member device".into() });
        }
        match &op {
            Op::Genesis { .. } => return Err(Error::BadRequest("a mesh has one genesis".into())),
            Op::Add { device, .. } => {
                if self.state.removed.contains(device) {
                    return Err(Error::Conflict("that device was removed; it joins again with a new key".into()));
                }
                if self.is_member(device) {
                    return Err(Error::Conflict("that device is already in the mesh".into()));
                }
            }
            Op::Name { device, .. } => {
                if !self.is_member(device) {
                    return Err(Error::BadRequest("only a device in the mesh can be named".into()));
                }
            }
            Op::Remove { .. } | Op::Revoke { .. } => {}
        }
        let body = Body { mesh: Some(self.mesh), parents: self.state.heads.clone(), at: crate::now(), op };
        let signed = Signed::sign(key, ENTRY_DOMAIN, &body)?;
        let entry = Entry::open(signed.clone())?;
        if self.insert(std::iter::once(entry)).is_empty() {
            return Err(Error::Internal("the list did not take its own entry".into()));
        }
        Ok(signed)
    }

    pub fn mesh(&self) -> MeshId {
        self.mesh
    }

    /// The device that made the mesh.
    pub fn founder(&self) -> DeviceId {
        self.founder
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn heads(&self) -> &[EntryId] {
        &self.state.heads
    }

    pub fn is_member(&self, device: &DeviceId) -> bool {
        self.state.members.contains_key(device)
    }

    pub fn member(&self, device: &DeviceId) -> Option<&Member> {
        self.state.members.get(device)
    }

    pub fn is_revoked(&self, note: &NoteId) -> bool {
        self.state.revoked.contains(note)
    }

    /// The members `device` added.
    pub fn added_by(&self, device: &DeviceId) -> Vec<DeviceId> {
        self.state.members.iter().filter(|(_, m)| m.added_by.as_ref() == Some(device)).map(|(d, _)| *d).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many entries are waiting for parents this list has not seen.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    pub fn contains(&self, id: &EntryId) -> bool {
        self.entries.contains_key(id)
    }

    /// Every entry, parents first.
    pub fn entries(&self) -> Vec<Signed> {
        self.order.iter().map(|id| self.entries[id].signed.clone()).collect()
    }

    /// Order the entries, work out which count, and drop the ones that never can. Returns the
    /// ids dropped.
    #[allow(clippy::needless_range_loop)]
    fn recompute(&mut self) -> Vec<EntryId> {
        let order = topological(&self.entries);
        let index: HashMap<EntryId, usize> = order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let n = order.len();
        let entries: Vec<&Entry> = order.iter().map(|id| &self.entries[id]).collect();

        // Strict ancestors of each entry, as bitsets over `order`.
        let words = n.div_ceil(64);
        let mut ancestors = vec![vec![0u64; words]; n];
        for i in 0..n {
            for parent in &entries[i].body.parents {
                let p = index[parent];
                let (head, tail) = ancestors.split_at_mut(i);
                for (w, word) in tail[0].iter_mut().enumerate() {
                    *word |= head[p][w];
                }
                tail[0][p / 64] |= 1 << (p % 64);
            }
        }
        let is_ancestor = |a: usize, of: usize| ancestors[of][a / 64] & (1 << (a % 64)) != 0;

        // Rule 1, read causally and ignoring rule 3: was the author a member, as its parents show?
        // Removals are judged by this alone (rule 2).
        let mut raw = vec![false; n];
        for i in 0..n {
            raw[i] = self.member_before(i, &entries, &raw, &is_ancestor);
        }

        // Rule 3: what a removed device wrote that its remover had not seen.
        let mut voided = vec![false; n];
        for r in 0..n {
            let Op::Remove { devices } = &entries[r].body.op else { continue };
            if !raw[r] {
                continue;
            }
            for (i, entry) in entries.iter().enumerate() {
                if i != r
                    && devices.contains(&entry.author)
                    && !matches!(entry.body.op, Op::Remove { .. })
                    && !is_ancestor(i, r)
                {
                    voided[i] = true;
                }
            }
        }

        // What counts: removals by rule 2, everything else by rules 1 and 3, membership itself
        // coming only from additions that count.
        let mut valid = vec![false; n];
        for i in 0..n {
            valid[i] = match entries[i].body.op {
                Op::Remove { .. } => raw[i],
                _ => !voided[i] && self.member_before(i, &entries, &valid, &is_ancestor),
            };
        }

        let mut state = State::default();
        for i in 0..n {
            let entry = entries[i];
            if !valid[i] {
                continue;
            }
            match &entry.body.op {
                Op::Genesis { name } => {
                    state
                        .members
                        .insert(entry.author, Member { name: name.clone(), added_by: None, added_at: entry.body.at });
                }
                Op::Add { device, name } => {
                    state.members.entry(*device).or_insert(Member {
                        name: name.clone(),
                        added_by: Some(entry.author),
                        added_at: entry.body.at,
                    });
                }
                Op::Remove { devices } => state.removed.extend(devices.iter().copied()),
                Op::Name { device, name } => {
                    if let Some(member) = state.members.get_mut(device) {
                        member.name = name.clone();
                    }
                }
                Op::Revoke { note } => {
                    state.revoked.insert(*note);
                }
            }
        }
        state.members.retain(|device, _| !state.removed.contains(device));

        // An entry whose author was never a member, as its own parents show, can never count.
        let dropped: Vec<EntryId> = (0..n).filter(|&i| !raw[i]).map(|i| order[i]).collect();
        for id in &dropped {
            self.entries.remove(id);
        }
        let mut has_child: BTreeSet<EntryId> = BTreeSet::new();
        for entry in self.entries.values() {
            has_child.extend(entry.body.parents.iter().copied());
        }
        let mut heads: Vec<EntryId> = self.entries.keys().filter(|id| !has_child.contains(id)).copied().collect();
        heads.sort();
        state.heads = heads;
        self.order = order.into_iter().filter(|id| self.entries.contains_key(id)).collect();
        self.state = state;
        dropped
    }

    /// Was entry `i`'s author a member when it wrote it: the founder, or added by an earlier
    /// entry that `counts`, and removed by no earlier entry that counts.
    fn member_before(
        &self,
        i: usize,
        entries: &[&Entry],
        counts: &[bool],
        is_ancestor: &impl Fn(usize, usize) -> bool,
    ) -> bool {
        let author = entries[i].author;
        if matches!(entries[i].body.op, Op::Genesis { .. }) {
            return true;
        }
        let mut added = author == self.founder;
        for j in 0..i {
            if !counts[j] || !is_ancestor(j, i) {
                continue;
            }
            match &entries[j].body.op {
                Op::Add { device, .. } if *device == author => added = true,
                Op::Remove { devices } if devices.contains(&author) => return false,
                _ => {}
            }
        }
        added
    }
}

/// Parents before children; among entries whose parents are all placed, the lowest id first.
fn topological(entries: &HashMap<EntryId, Entry>) -> Vec<EntryId> {
    let mut children: HashMap<EntryId, Vec<EntryId>> = HashMap::new();
    let mut waiting: HashMap<EntryId, usize> = HashMap::new();
    for entry in entries.values() {
        waiting.insert(entry.id, entry.body.parents.len());
        for parent in &entry.body.parents {
            children.entry(*parent).or_default().push(entry.id);
        }
    }
    let mut ready: BTreeSet<EntryId> = waiting.iter().filter(|(_, n)| **n == 0).map(|(id, _)| *id).collect();
    let mut order = Vec::with_capacity(entries.len());
    while let Some(id) = ready.pop_first() {
        order.push(id);
        for child in children.get(&id).into_iter().flatten() {
            let n = waiting.get_mut(child).expect("every child is an entry");
            *n -= 1;
            if *n == 0 {
                ready.insert(*child);
            }
        }
    }
    order
}
