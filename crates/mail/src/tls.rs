//! Gemeinsame TLS-Konfiguration: rustls mit dem macOS-Zertifikatsspeicher.
//! Damit gelten dieselben Vertrauensregeln wie in Safari.

use std::sync::{Arc, OnceLock};

use kolibri_core::{Error, ErrorKind, Result};
use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

static CONFIG: OnceLock<std::result::Result<Arc<ClientConfig>, String>> = OnceLock::new();

fn config() -> Result<Arc<ClientConfig>> {
    CONFIG
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| e.to_string())?
                .with_platform_verifier()
                .map(|b| Arc::new(b.with_no_client_auth()))
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(|e| Error::new(ErrorKind::Tls, e))
}

/// Baut eine TLS-Verbindung über einen bestehenden TCP-Stream auf.
pub async fn wrap(host: &str, tcp: TcpStream) -> Result<TlsStream<TcpStream>> {
    let name = rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|e| {
        Error::new(
            ErrorKind::Invalid,
            format!("Ungültiger Hostname {host}: {e}"),
        )
    })?;
    TlsConnector::from(config()?)
        .connect(name, tcp)
        .await
        .map_err(|e| Error::new(ErrorKind::Tls, format!("{host}: {e}")))
}
