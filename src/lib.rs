//! erisauth: sign apps in to Eris services from a device you already trust.
//!
//! [Home](home) is ErisAuth's plugin in ErisDB. It holds full authority and
//! approves a pending pairing for a registered ErisAuth device that brings
//! the passphrase, so a device with little authority of its own can still
//! approve what an app needs once the person proves they are there.

pub mod erisdb;
pub mod home;
pub mod plugin;
