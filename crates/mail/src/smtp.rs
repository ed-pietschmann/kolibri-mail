//! SMTP über `lettre`: Anmeldetest und Versand fertig gebauter Nachrichten.

use std::time::Duration;

use kolibri_core::{Error, ErrorKind, Result, Security, ServerConfig};
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

const TIMEOUT: Duration = Duration::from_secs(20);

fn smtp_err(host: &str, err: &lettre::transport::smtp::Error) -> Error {
    let detail = format!("{host}: {err}");
    let auth_failed = err
        .status()
        .is_some_and(|code| matches!(code.to_string().as_str(), "535" | "534" | "530"));
    let kind = if auth_failed {
        ErrorKind::Auth
    } else if err.is_timeout() {
        ErrorKind::Timeout
    } else if err.is_tls() {
        ErrorKind::Tls
    } else if err.is_transient() {
        // 4xx: vorübergehend – später erneut versuchen.
        ErrorKind::Connect
    } else if err.is_permanent() {
        ErrorKind::Protocol
    } else {
        ErrorKind::Connect
    };
    Error::new(kind, detail)
}

pub(crate) fn transport(
    cfg: &ServerConfig,
    password: &str,
) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    if !cfg.is_allowed() {
        return Err(Error::new(
            ErrorKind::Invalid,
            "Unverschlüsselte Verbindungen sind nur zu localhost erlaubt",
        ));
    }
    let builder = match cfg.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host),
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host),
        Security::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            &cfg.host,
        )),
    }
    .map_err(|e| smtp_err(&cfg.host, &e))?;
    Ok(builder
        .port(cfg.port)
        .credentials(Credentials::new(cfg.username.clone(), password.to_owned()))
        .timeout(Some(TIMEOUT))
        .build())
}

/// Verbindet sich und meldet sich an, ohne etwas zu senden.
pub async fn test_login(cfg: &ServerConfig, password: &str) -> Result<()> {
    let transport = transport(cfg, password)?;
    match transport.test_connection().await {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::new(
            ErrorKind::Connect,
            format!("{}: Server antwortet nicht", cfg.host),
        )),
        Err(e) => Err(smtp_err(&cfg.host, &e)),
    }
}

/// Verschickt eine fertig gebaute Nachricht.
pub async fn send_raw(
    cfg: &ServerConfig,
    password: &str,
    from: &str,
    to: &[String],
    raw: &[u8],
) -> Result<()> {
    let parse = |a: &str| {
        a.parse()
            .map_err(|_| Error::new(ErrorKind::Invalid, format!("Ungültige Adresse: {a}")))
    };
    let recipients = to.iter().map(|a| parse(a)).collect::<Result<Vec<_>>>()?;
    let envelope = Envelope::new(Some(parse(from)?), recipients)
        .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
    let transport = transport(cfg, password)?;
    transport
        .send_raw(&envelope, raw)
        .await
        .map(|_| ())
        .map_err(|e| smtp_err(&cfg.host, &e))
}
