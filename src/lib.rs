//! erisauth: sign apps in to Eris services from a device you already trust.
//!
//! On a [device](device), ErisAuth is an ErisDB installation of its own that
//! apps ask instead of showing the person a ticket. It checks that the app
//! redeeming each ticket is the app that asked, puts the request to the
//! person, and approves it: by itself within its ceiling, or through home.
//!
//! [Home](home) is ErisAuth's plugin in ErisDB. It holds full authority and
//! approves a pending pairing for a registered ErisAuth device that brings
//! the passphrase, so a device with little authority of its own can still
//! approve what an app needs once the person proves they are there.

pub mod device;
pub mod erisdb;
pub mod home;
pub mod plugin;
