//! eon: the core of each device in an Eris mesh.
//!
//! A person's devices form one mesh, and ErisOverlayNetwork is what each of them runs: it holds the
//! device's key and runs the device's [node](erisoverlaynetwork::mesh::Node), which keeps the
//! signed list of the person's devices. From any device in the mesh the person adds the next
//! one (a ticket, a fingerprint on both screens, one approval), renames or removes devices, and
//! approves the apps that ask: a mesh service with one tap, ErisDB with a tap and the ErisDB
//! passphrase, which ErisDB itself checks. See [`device`].

pub mod device;
