//! Pure application data types and rules.
//!
//! Domain modules intentionally do not depend on GTK, zbus, or operating
//! system APIs. This keeps ordering, classification, and presentation input
//! deterministic and straightforward to test.

pub(crate) mod audio;
pub(crate) mod bluetooth;
pub(crate) mod network;
pub(crate) mod power;
pub(crate) mod vpn;
