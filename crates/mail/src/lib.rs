//! IMAP/SMTP-Zugang und automatische Servererkennung.

pub mod autoconfig;
pub mod compose;
pub mod imap;
pub mod parse;
pub mod sanitize;
pub mod smtp;
mod tls;
pub mod utf7;

pub use autoconfig::{Discovered, Source, discover};
pub use imap::ImapSession;
