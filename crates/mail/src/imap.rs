//! IMAP-Zugang über `async-imap`.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_imap::types::{Flag, NameAttribute};
use futures::TryStreamExt;
use kolibri_core::{
    Error, ErrorKind, Flags, FolderRole, MailProvider, RemoteFolder, RemoteMessage, Result,
    Security, ServerConfig,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::{parse, tls, utf7};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Verbindung: Klartext (nur vor STARTTLS bzw. zu localhost) oder TLS.
#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Eine angemeldete IMAP-Sitzung.
pub struct ImapSession {
    session: async_imap::Session<Stream>,
    /// Aktuell geöffneter Ordner und ob schreibend (SELECT) statt nur lesend (EXAMINE).
    selected: Option<(String, bool)>,
    /// Server-Fähigkeiten (MOVE, UIDPLUS …), einmal pro Sitzung abgefragt.
    capabilities: Option<Vec<String>>,
}

/// Zustand eines Ordners auf dem Server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderStatus {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub exists: u32,
}

/// Höchstens so viele UIDs pro FETCH-Befehl.
pub const FETCH_BATCH: usize = 200;

/// Kompakte UID-Menge für IMAP: `[1,2,3,7,9,10]` → `1:3,7,9:10`.
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted: Vec<u32> = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out = String::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        if !out.is_empty() {
            out.push(',');
        }
        if start == end {
            out.push_str(&start.to_string());
        } else {
            out.push_str(&format!("{start}:{end}"));
        }
        i += 1;
    }
    out
}

fn flags_of(fetch: &async_imap::types::Fetch) -> Flags {
    let mut f = Flags::default();
    for flag in fetch.flags() {
        match flag {
            Flag::Seen => f.seen = true,
            Flag::Flagged => f.flagged = true,
            Flag::Answered => f.answered = true,
            Flag::Draft => f.draft = true,
            _ => {}
        }
    }
    f
}

/// Führt ein Future mit Zeitlimit aus.
pub(crate) async fn timed<T>(
    limit: Duration,
    what: &str,
    fut: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(limit, fut)
        .await
        .map_err(|_| Error::new(ErrorKind::Timeout, what.to_owned()))?
}

pub(crate) async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream> {
    timed(CONNECT_TIMEOUT, &format!("{host}:{port}"), async {
        TcpStream::connect((host, port))
            .await
            .map_err(|e| Error::new(ErrorKind::Connect, format!("{host}:{port}: {e}")))
    })
    .await
}

fn imap_err(err: async_imap::error::Error) -> Error {
    use async_imap::error::Error as E;
    match err {
        E::No(msg) => Error::new(ErrorKind::Auth, msg),
        E::Io(e) => Error::new(ErrorKind::Connect, e.to_string()),
        E::ConnectionLost => Error::new(ErrorKind::Connect, "Verbindung verloren"),
        other => Error::new(ErrorKind::Protocol, other.to_string()),
    }
}

/// Liest die Begrüßung des Servers (muss vor jedem Befehl passieren).
async fn read_greeting(client: &mut async_imap::Client<Stream>) -> Result<()> {
    match client.read_response().await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(Error::new(
            ErrorKind::Protocol,
            "Keine Begrüßung vom Server",
        )),
        Err(e) => Err(Error::new(ErrorKind::Connect, e.to_string())),
    }
}

impl ImapSession {
    /// Verbindet und meldet sich an.
    pub async fn connect(cfg: &ServerConfig, password: &str) -> Result<Self> {
        if !cfg.is_allowed() {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Unverschlüsselte Verbindungen sind nur zu localhost erlaubt",
            ));
        }
        let client = timed(CONNECT_TIMEOUT * 2, &cfg.host, Self::open(cfg)).await?;
        let session = timed(COMMAND_TIMEOUT, "LOGIN", async {
            client
                .login(&cfg.username, password)
                .await
                .map_err(|(e, _)| imap_err(e))
        })
        .await?;
        Ok(Self {
            session,
            selected: None,
            capabilities: None,
        })
    }

    async fn open(cfg: &ServerConfig) -> Result<async_imap::Client<Stream>> {
        let tcp = tcp_connect(&cfg.host, cfg.port).await?;
        match cfg.security {
            Security::Tls => {
                let tls = tls::wrap(&cfg.host, tcp).await?;
                let mut client = async_imap::Client::new(Stream::Tls(Box::new(tls)));
                read_greeting(&mut client).await?;
                Ok(client)
            }
            Security::StartTls => {
                let mut client = async_imap::Client::new(Stream::Plain(tcp));
                read_greeting(&mut client).await?;
                client
                    .run_command_and_check_ok("STARTTLS", None)
                    .await
                    .map_err(|e| Error::new(ErrorKind::Tls, format!("STARTTLS: {e}")))?;
                // Den rohen TCP-Stream zurückholen und in TLS einpacken.
                let Stream::Plain(tcp) = client.into_inner() else {
                    return Err(Error::new(
                        ErrorKind::Internal,
                        "STARTTLS auf TLS-Verbindung",
                    ));
                };
                let tls = tls::wrap(&cfg.host, tcp).await?;
                Ok(async_imap::Client::new(Stream::Tls(Box::new(tls))))
            }
            Security::None => {
                let mut client = async_imap::Client::new(Stream::Plain(tcp));
                read_greeting(&mut client).await?;
                Ok(client)
            }
        }
    }
}

impl ImapSession {
    /// Öffnet einen Ordner nur lesend (EXAMINE) und liefert seinen Zustand.
    pub async fn examine(&mut self, path: &str) -> Result<FolderStatus> {
        let mailbox = timed(COMMAND_TIMEOUT, "EXAMINE", async {
            self.session
                .examine(utf7::encode(path))
                .await
                .map_err(imap_err)
        })
        .await?;
        self.selected = Some((path.to_owned(), false));
        Ok(FolderStatus {
            uid_validity: mailbox.uid_validity.unwrap_or(0),
            uid_next: mailbox.uid_next.unwrap_or(0),
            exists: mailbox.exists,
        })
    }

    async fn ensure_selected(&mut self, path: &str) -> Result<()> {
        if self.selected.as_ref().map(|(p, _)| p.as_str()) != Some(path) {
            self.examine(path).await?;
        }
        Ok(())
    }

    /// UIDs aller Nachrichten seit `since` (Unix-Sekunden), aufsteigend.
    pub async fn uids_since(&mut self, path: &str, since: i64) -> Result<Vec<u32>> {
        self.ensure_selected(path).await?;
        let date = chrono::DateTime::from_timestamp(since, 0)
            .unwrap_or_default()
            .format("%d-%b-%Y")
            .to_string();
        let set = timed(COMMAND_TIMEOUT, "SEARCH", async {
            self.session
                .uid_search(format!("SINCE {date}"))
                .await
                .map_err(imap_err)
        })
        .await?;
        let mut uids: Vec<u32> = set.into_iter().collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// Kopfdaten für die angegebenen UIDs (Aufrufer teilt in Häppchen auf).
    pub async fn fetch_headers(&mut self, path: &str, uids: &[u32]) -> Result<Vec<RemoteMessage>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_selected(path).await?;
        let set = uid_set(uids);
        timed(COMMAND_TIMEOUT * 2, "FETCH", async {
            let fetches: Vec<_> = self
                .session
                .uid_fetch(
                    &set,
                    "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])",
                )
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            Ok(fetches
                .iter()
                .filter_map(|f| {
                    let uid = f.uid?;
                    Some(parse::parse_header(
                        uid,
                        f.header().unwrap_or_default(),
                        flags_of(f),
                        f.internal_date().map(|d| d.timestamp()),
                        f.size.unwrap_or(0),
                    ))
                })
                .collect())
        })
        .await
    }

    /// Aktuelle Flags für einen UID-Bereich.
    pub async fn fetch_flags(&mut self, path: &str, uids: &[u32]) -> Result<Vec<(u32, Flags)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_selected(path).await?;
        let set = uid_set(uids);
        timed(COMMAND_TIMEOUT * 2, "FETCH FLAGS", async {
            let fetches: Vec<_> = self
                .session
                .uid_fetch(&set, "(UID FLAGS)")
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            Ok(fetches
                .iter()
                .filter_map(|f| Some((f.uid?, flags_of(f))))
                .collect())
        })
        .await
    }

    /// Komplette Nachricht (ohne sie als gelesen zu markieren).
    pub async fn fetch_raw(&mut self, path: &str, uid: u32) -> Result<Vec<u8>> {
        self.ensure_selected(path).await?;
        timed(Duration::from_secs(120), "FETCH BODY", async {
            let fetches: Vec<_> = self
                .session
                .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            fetches
                .iter()
                .find(|f| f.uid == Some(uid))
                .and_then(|f| f.body())
                .map(<[u8]>::to_vec)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::NotFound,
                        format!("Nachricht {uid} nicht mehr auf dem Server"),
                    )
                })
        })
        .await
    }

    /// Öffnet einen Ordner schreibend (für Aktionen).
    async fn ensure_writable(&mut self, path: &str) -> Result<()> {
        if self.selected.as_ref() == Some(&(path.to_owned(), true)) {
            return Ok(());
        }
        timed(COMMAND_TIMEOUT, "SELECT", async {
            self.session
                .select(utf7::encode(path))
                .await
                .map_err(imap_err)
        })
        .await?;
        self.selected = Some((path.to_owned(), true));
        Ok(())
    }

    async fn has_capability(&mut self, name: &str) -> Result<bool> {
        if self.capabilities.is_none() {
            let caps = timed(COMMAND_TIMEOUT, "CAPABILITY", async {
                self.session.capabilities().await.map_err(imap_err)
            })
            .await?;
            let list = ["MOVE", "UIDPLUS", "IDLE"]
                .iter()
                .filter(|c| caps.has_str(c))
                .map(|c| (*c).to_owned())
                .collect();
            self.capabilities = Some(list);
        }
        Ok(self
            .capabilities
            .as_ref()
            .is_some_and(|c| c.iter().any(|x| x == name)))
    }

    /// Setzt (`add = true`) oder entfernt IMAP-Flags wie `\Seen`, `\Flagged`.
    pub async fn store_flags(
        &mut self,
        path: &str,
        uids: &[u32],
        flag: &str,
        add: bool,
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        self.ensure_writable(path).await?;
        let set = uid_set(uids);
        let query = format!("{}FLAGS.SILENT ({flag})", if add { '+' } else { '-' });
        timed(COMMAND_TIMEOUT, "STORE", async {
            let _: Vec<_> = self
                .session
                .uid_store(&set, &query)
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            Ok(())
        })
        .await
    }

    /// Entfernt als gelöscht markierte Nachrichten – nur die angegebenen, wenn
    /// der Server UIDPLUS kann, sonst alle markierten im Ordner.
    async fn expunge(&mut self, uids: &[u32]) -> Result<()> {
        let uidplus = self.has_capability("UIDPLUS").await?;
        let set = uid_set(uids);
        timed(COMMAND_TIMEOUT, "EXPUNGE", async {
            if uidplus {
                let _: Vec<_> = self
                    .session
                    .uid_expunge(&set)
                    .await
                    .map_err(imap_err)?
                    .try_collect()
                    .await
                    .map_err(imap_err)?;
            } else {
                let _: Vec<_> = self
                    .session
                    .expunge()
                    .await
                    .map_err(imap_err)?
                    .try_collect()
                    .await
                    .map_err(imap_err)?;
            }
            Ok(())
        })
        .await
    }

    /// Verschiebt Nachrichten (MOVE, sonst COPY + löschen).
    pub async fn move_messages(&mut self, path: &str, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        self.ensure_writable(path).await?;
        let set = uid_set(uids);
        let target = utf7::encode(dest);
        if self.has_capability("MOVE").await? {
            return timed(COMMAND_TIMEOUT, "MOVE", async {
                self.session.uid_mv(&set, &target).await.map_err(imap_err)
            })
            .await;
        }
        timed(COMMAND_TIMEOUT, "COPY", async {
            self.session.uid_copy(&set, &target).await.map_err(imap_err)
        })
        .await?;
        self.store_flags(path, uids, "\\Deleted", true).await?;
        self.expunge(uids).await
    }

    /// Löscht Nachrichten endgültig.
    pub async fn delete_permanently(&mut self, path: &str, uids: &[u32]) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        self.store_flags(path, uids, "\\Deleted", true).await?;
        self.expunge(uids).await
    }

    /// Legt einen Ordner an; existiert er schon, ist das kein Fehler.
    pub async fn create_folder(&mut self, path: &str) -> Result<()> {
        let result = timed(COMMAND_TIMEOUT, "CREATE", async {
            self.session
                .create(utf7::encode(path))
                .await
                .map_err(imap_err)
        })
        .await;
        match result {
            Err(e) if e.detail.to_ascii_uppercase().contains("EXIST") => Ok(()),
            other => other,
        }
    }

    /// Legt eine Nachricht in einem Ordner ab (z. B. „Gesendet“, „Entwürfe“).
    pub async fn append(&mut self, path: &str, flags: &str, raw: &[u8]) -> Result<()> {
        let mailbox = utf7::encode(path);
        timed(Duration::from_secs(120), "APPEND", async {
            self.session
                .append(&mailbox, Some(flags), None, raw)
                .await
                .map_err(imap_err)
        })
        .await
    }

    /// UIDs aller Nachrichten mit dieser Message-ID (ohne Klammern) im Ordner.
    ///
    /// Nicht über `SEARCH HEADER`, weil manche Server (z. B. Stalwart) diese
    /// Kopfzeile nicht durchsuchen. Stattdessen die ENVELOPE-Daten lesen –
    /// für Entwurfs- und Gesendet-Ordner schnell genug.
    pub async fn uids_with_message_id(&mut self, path: &str, message_id: &str) -> Result<Vec<u32>> {
        let wanted = message_id.trim().trim_matches(|c| c == '<' || c == '>');
        if self.examine(path).await?.exists == 0 {
            return Ok(Vec::new());
        }
        timed(COMMAND_TIMEOUT * 2, "FETCH ENVELOPE", async {
            let fetches: Vec<_> = self
                .session
                .uid_fetch("1:*", "(UID ENVELOPE)")
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            let mut uids: Vec<u32> = fetches
                .iter()
                .filter_map(|f| {
                    let id = f.envelope()?.message_id.as_ref()?;
                    let id = String::from_utf8_lossy(id);
                    (id.trim().trim_matches(|c| c == '<' || c == '>') == wanted).then_some(f.uid?)
                })
                .collect();
            uids.sort_unstable();
            Ok(uids)
        })
        .await
    }

    /// Kann der Server Push (IMAP IDLE, RFC 2177)?
    pub async fn supports_idle(&mut self) -> Result<bool> {
        self.has_capability("IDLE").await
    }

    /// Wartet per IDLE auf Änderungen im Ordner, höchstens `timeout`.
    /// Liefert die Sitzung zurück und ob sich etwas getan hat.
    /// Der Server meldet sich von selbst – im Warten fällt keine CPU-Last an.
    pub async fn idle_wait(mut self, path: &str, timeout: Duration) -> Result<(Self, bool)> {
        self.examine(path).await?;
        let Self {
            session,
            selected,
            capabilities,
        } = self;
        let mut handle = session.idle();
        timed(COMMAND_TIMEOUT, "IDLE", async {
            handle.init().await.map_err(imap_err)
        })
        .await?;
        let changed = {
            let (wait, _stop) = handle.wait_with_timeout(timeout);
            matches!(
                wait.await.map_err(imap_err)?,
                async_imap::extensions::idle::IdleResponse::NewData(_)
            )
        };
        let session = timed(COMMAND_TIMEOUT, "DONE", async {
            handle.done().await.map_err(imap_err)
        })
        .await?;
        Ok((
            Self {
                session,
                selected,
                capabilities,
            },
            changed,
        ))
    }

    /// Prüft, ob die Verbindung noch lebt (für wiederverwendete Sitzungen).
    pub async fn noop(&mut self) -> Result<()> {
        timed(Duration::from_secs(10), "NOOP", async {
            self.session.noop().await.map_err(imap_err)
        })
        .await
    }
}

fn role_from_attributes(attrs: &[NameAttribute<'_>]) -> Option<FolderRole> {
    attrs.iter().find_map(|a| match a {
        NameAttribute::All => Some(FolderRole::All),
        NameAttribute::Archive => Some(FolderRole::Archive),
        NameAttribute::Drafts => Some(FolderRole::Drafts),
        NameAttribute::Flagged => Some(FolderRole::Flagged),
        NameAttribute::Junk => Some(FolderRole::Junk),
        NameAttribute::Sent => Some(FolderRole::Sent),
        NameAttribute::Trash => Some(FolderRole::Trash),
        _ => None,
    })
}

impl MailProvider for ImapSession {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>> {
        timed(COMMAND_TIMEOUT, "LIST", async {
            let names: Vec<_> = self
                .session
                .list(Some(""), Some("*"))
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            let mut folders: Vec<RemoteFolder> = names
                .iter()
                .map(|n| {
                    let path = utf7::decode(n.name());
                    let selectable = !n.attributes().iter().any(|a| {
                        matches!(a, NameAttribute::NoSelect)
                            || matches!(a, NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\NonExistent"))
                    });
                    let role = if path.eq_ignore_ascii_case("INBOX") {
                        Some(FolderRole::Inbox)
                    } else {
                        role_from_attributes(n.attributes())
                    };
                    RemoteFolder {
                        path,
                        delimiter: n.delimiter().map(str::to_owned),
                        role,
                        selectable,
                    }
                })
                .collect();
            assign_missing_roles(&mut folders);
            Ok(folders)
        })
        .await
    }

    async fn logout(mut self) -> Result<()> {
        timed(Duration::from_secs(5), "LOGOUT", async {
            self.session.logout().await.map_err(imap_err)
        })
        .await
    }
}

/// Server ohne SPECIAL-USE: Rollen am Namen erkennen, aber jede Rolle nur einmal
/// vergeben und nur an Ordner der obersten Ebene (oder direkt unter INBOX).
pub fn assign_missing_roles(folders: &mut [RemoteFolder]) {
    let taken: Vec<FolderRole> = folders.iter().filter_map(|f| f.role).collect();
    let mut taken = taken;
    for f in folders
        .iter_mut()
        .filter(|f| f.role.is_none() && f.selectable)
    {
        let top_level = match &f.delimiter {
            Some(d) if !d.is_empty() => {
                let parts: Vec<&str> = f.path.split(d.as_str()).collect();
                parts.len() == 1
                    || (parts.len() == 2
                        && parts
                            .first()
                            .is_some_and(|p| p.eq_ignore_ascii_case("INBOX")))
            }
            _ => true,
        };
        if !top_level {
            continue;
        }
        if let Some(role) = FolderRole::guess_from_name(f.name())
            && !taken.contains(&role)
        {
            f.role = Some(role);
            taken.push(role);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, role: Option<FolderRole>) -> RemoteFolder {
        RemoteFolder {
            path: path.into(),
            delimiter: Some(".".into()),
            role,
            selectable: true,
        }
    }

    #[test]
    fn guesses_roles_only_once_and_only_top_level() {
        let mut folders = vec![
            f("INBOX", Some(FolderRole::Inbox)),
            f("INBOX.Sent", None),
            f("Sent", None),
            f("Projekte.Papierkorb", None),
            f("Papierkorb", None),
        ];
        assign_missing_roles(&mut folders);
        let roles: Vec<_> = folders.iter().map(|f| f.role).collect();
        assert_eq!(
            roles,
            [
                Some(FolderRole::Inbox),
                Some(FolderRole::Sent),
                None,
                None,
                Some(FolderRole::Trash)
            ]
        );
    }

    #[test]
    fn compresses_uid_sets() {
        assert_eq!(uid_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
        assert_eq!(uid_set(&[5]), "5");
        assert_eq!(uid_set(&[3, 1, 2, 2]), "1:3");
        assert_eq!(uid_set(&[]), "");
    }

    #[test]
    fn special_use_wins_over_name() {
        let mut folders = vec![f("Gesendet", None), f("Ausgang", Some(FolderRole::Sent))];
        assign_missing_roles(&mut folders);
        assert_eq!(folders[0].role, None);
    }
}
