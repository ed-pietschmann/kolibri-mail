//! Verfassen, Senden, Entwürfe, Signatur und Anhänge öffnen.
//!
//! Senden und Entwürfe laufen über den Postausgang (`outbox`): Die fertige
//! Nachricht wird sofort gespeichert, der Arbeiter überträgt sie, sobald eine
//! Verbindung besteht. So geht auch offline nichts verloren.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use kolibri_core::{
    Account, AccountId, Action, ComposeAttachment, ComposeDraft, ComposeKind, Error, ErrorKind,
    FolderRole, MAX_ATTACHMENTS_BYTES, MessageId, Result, SecretStore,
};
use kolibri_mail::compose::{
    self, FilePart, fill_template, format_addresses, new_message_id, prefixed_subject,
    reply_references, wrap_html,
};
use kolibri_mail::parse::{attachment_data, safe_file_name};
use kolibri_store::{DraftMeta, OutboxItem, OutboxKind, Signature, Store, Template};
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::accounts::AppState;
use crate::keychain::account_key;
use crate::sync::{SessionPool, is_connection_error};

/// Entwurfsordner-Name, falls das Konto keinen hat.
const DRAFTS_NAME: &str = "Drafts";
const SENT_NAME: &str = "Sent";

fn strip_brackets(id: &str) -> String {
    id.trim().trim_matches(|c| c == '<' || c == '>').to_owned()
}

fn display(name: &str, addr: &str) -> String {
    if name.trim().is_empty() {
        addr.to_owned()
    } else {
        format!("{name} <{addr}>")
    }
}

/// Signatur als eigener Block im Editor (der Editor zeigt ihn als festes Element).
pub fn signature_block(sig: Option<&Signature>) -> String {
    match sig {
        Some(s) if !s.html.trim().is_empty() => {
            format!("<div data-kolibri-signature=\"{}\">{}</div>", s.id, s.html)
        }
        _ => String::new(),
    }
}

impl AppState {
    /// Fertige HTML-Seite der Mail: eigener Code, Vorlage oder schlichte Hülle.
    pub fn render_document(&self, account: &Account, draft: &ComposeDraft) -> Result<String> {
        if let Some(code) = draft.code.as_deref().filter(|c| !c.trim().is_empty()) {
            return Ok(fill_template(code, "", account, &draft.subject));
        }
        if let Some(t) = draft
            .template_id
            .map(|id| self.store.template(id))
            .transpose()?
            .flatten()
        {
            return Ok(fill_template(&t.html, &draft.html, account, &draft.subject));
        }
        Ok(wrap_html(&draft.html))
    }

    fn default_account(&self, wanted: Option<AccountId>) -> Result<Account> {
        match wanted {
            Some(id) => self.store.account(id),
            None => self
                .store
                .accounts()?
                .into_iter()
                .next()
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "Noch kein Konto eingerichtet")),
        }
    }

    /// Vorbefüllter Entwurf für Neu/Antworten/Weiterleiten/Bearbeiten.
    pub async fn prepare_compose(&self, kind: ComposeKind) -> Result<ComposeDraft> {
        let message = match kind {
            ComposeKind::New { account_id } => {
                let account = self.default_account(account_id)?;
                let sig = self.store.default_signature(account.id)?;
                return Ok(ComposeDraft {
                    account_id: account.id,
                    draft_id: new_message_id(&account.email),
                    to: String::new(),
                    cc: String::new(),
                    bcc: String::new(),
                    subject: String::new(),
                    html: format!("<p></p>{}", signature_block(sig.as_ref())),
                    attachments: vec![],
                    in_reply_to: None,
                    references: None,
                    reply_to_message: None,
                    signature_id: sig.as_ref().map(|s| s.id),
                    template_id: None,
                    code: None,
                });
            }
            ComposeKind::Reply { message_id }
            | ComposeKind::ReplyAll { message_id }
            | ComposeKind::Forward { message_id }
            | ComposeKind::EditDraft { message_id } => message_id,
        };

        let src = self.store.compose_source(message)?;
        let account = self.store.account(src.summary.account_id)?;
        let detail = self.get_message(message).await?;
        let body = &detail.body;
        let own = account.email.to_lowercase();
        let sender = display(&src.summary.from_name, &src.summary.from_addr);
        let signature = self.store.default_signature(account.id)?;
        let sig = signature_block(signature.as_ref());
        let originals = || {
            body.attachments
                .iter()
                .enumerate()
                .map(|(index, a)| ComposeAttachment::Original {
                    message_id: message,
                    index,
                    name: a.name.clone(),
                    size: a.size,
                })
                .collect::<Vec<_>>()
        };

        let mut draft = ComposeDraft {
            account_id: account.id,
            draft_id: new_message_id(&account.email),
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: String::new(),
            html: String::new(),
            attachments: vec![],
            in_reply_to: None,
            references: None,
            reply_to_message: None,
            signature_id: signature.as_ref().map(|s| s.id),
            template_id: None,
            code: None,
        };

        match kind {
            ComposeKind::Reply { .. } | ComposeKind::ReplyAll { .. } => {
                let primary = if src.reply_to.is_empty() {
                    vec![kolibri_core::Address {
                        name: src.summary.from_name.clone(),
                        addr: src.summary.from_addr.clone(),
                    }]
                } else {
                    src.reply_to.clone()
                };
                // Antwort auf die eigene Mail (z. B. aus „Gesendet“): an die
                // ursprünglichen Empfänger statt an sich selbst.
                let own_mail = src.summary.from_addr.to_lowercase() == own;
                let mut to = if own_mail { src.to.clone() } else { primary };
                let mut cc = Vec::new();
                if matches!(kind, ComposeKind::ReplyAll { .. }) {
                    if !own_mail {
                        to.extend(src.to.iter().cloned());
                    }
                    cc = src.cc.clone();
                }
                draft.to = format_addresses(&to, &[own.as_str()]);
                let to_lower: Vec<String> = to.iter().map(|a| a.addr.to_lowercase()).collect();
                let mut exclude: Vec<&str> = to_lower.iter().map(String::as_str).collect();
                exclude.push(own.as_str());
                draft.cc = format_addresses(&cc, &exclude);
                draft.subject = prefixed_subject("Re", &src.summary.subject);
                if let Some(mid) = &src.message_id {
                    draft.in_reply_to = Some(format!("<{}>", strip_brackets(mid)));
                    draft.references = Some(reply_references(
                        src.references.as_deref(),
                        src.in_reply_to.as_deref(),
                        mid,
                    ));
                }
                draft.reply_to_message = Some(message);
                draft.html = format!(
                    "<p></p>{sig}{}",
                    compose::reply_quote(&sender, src.summary.date, body)
                );
            }
            ComposeKind::Forward { .. } => {
                draft.subject = prefixed_subject("Fwd", &src.summary.subject);
                draft.attachments = originals();
                draft.html = format!(
                    "<p></p>{sig}{}",
                    compose::forward_block(
                        &sender,
                        &src.summary.subject,
                        src.summary.date,
                        &format_addresses(&src.to, &[]),
                        body
                    )
                );
            }
            ComposeKind::EditDraft { .. } => {
                if let Some(mid) = &src.message_id {
                    draft.draft_id = strip_brackets(mid);
                }
                draft.to = format_addresses(&src.to, &[]);
                draft.cc = format_addresses(&src.cc, &[]);
                draft.subject = src.summary.subject.clone();
                draft.html = compose::quotable(body);
                draft.attachments = originals();
                draft.in_reply_to = src.in_reply_to.clone();
                draft.references = src.references.clone();
                // Auf diesem Mac geschrieben? Dann Editor-Inhalt, Vorlage und Code
                // wiederherstellen (der Server kennt nur die fertige Mail).
                draft.signature_id = None;
                if let Some(meta) = self.store.draft_meta(&draft.draft_id)? {
                    draft.html = meta.editor_html;
                    draft.template_id = meta.template_id;
                    draft.signature_id = meta.signature_id;
                    draft.code = meta.code;
                }
            }
            ComposeKind::New { .. } => {}
        }
        Ok(draft)
    }

    /// Anhänge einlesen (Dateien von der Platte, Originale vom Server).
    async fn read_attachments(
        &self,
        account: &Account,
        draft: &ComposeDraft,
    ) -> Result<Vec<FilePart>> {
        let total: u64 = draft.attachments.iter().map(ComposeAttachment::size).sum();
        if total > MAX_ATTACHMENTS_BYTES {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Die Anhänge sind zusammen größer als 25 MB – viele Server lehnen das ab.",
            ));
        }
        // Originale je Nachricht nur einmal holen.
        let mut raws: BTreeMap<MessageId, Vec<u8>> = BTreeMap::new();
        let mut out = Vec::with_capacity(draft.attachments.len());
        for a in &draft.attachments {
            match a {
                ComposeAttachment::File { path, name, .. } => {
                    let data = std::fs::read(path).map_err(|e| {
                        Error::new(
                            ErrorKind::NotFound,
                            format!("{name} kann nicht gelesen werden: {e}"),
                        )
                    })?;
                    out.push(FilePart {
                        name: name.clone(),
                        data,
                    });
                }
                ComposeAttachment::Original {
                    message_id,
                    index,
                    name,
                    ..
                } => {
                    if !raws.contains_key(message_id) {
                        let raw = self.fetch_raw(*message_id).await?;
                        raws.insert(*message_id, raw);
                    }
                    let raw = raws.get(message_id).map(Vec::as_slice).unwrap_or_default();
                    let (_, _, data) = attachment_data(raw, *index).ok_or_else(|| {
                        Error::new(ErrorKind::NotFound, format!("Anhang {name} nicht gefunden"))
                    })?;
                    out.push(FilePart {
                        name: name.clone(),
                        data,
                    });
                }
            }
        }
        let _ = account;
        Ok(out)
    }

    /// Rohdaten einer Nachricht vom Server.
    async fn fetch_raw(&self, id: MessageId) -> Result<Vec<u8>> {
        let loc = self.store.message_location(id)?;
        let account = self.store.account(loc.summary.account_id)?;
        let password = self.secrets.password(&account_key(account.id))?;
        self.pool
            .fetch_raw(&account, &password, &loc.folder_path, loc.uid)
            .await
    }

    /// Nachricht bauen und in den Postausgang legen.
    pub async fn queue_send(&self, draft: &ComposeDraft) -> Result<()> {
        let account = self.store.account(draft.account_id)?;
        let files = self.read_attachments(&account, draft).await?;
        let document = self.render_document(&account, draft)?;
        let built = compose::build(&account, draft, &document, files, false)?;
        // Empfänger für künftige Adressvorschläge merken.
        let recipients: Vec<kolibri_core::Address> = [&draft.to, &draft.cc, &draft.bcc]
            .into_iter()
            .flat_map(|list| compose::parse_addresses(list).unwrap_or_default())
            .map(|m| kolibri_core::Address {
                name: m.name.unwrap_or_default(),
                addr: m.email.to_string(),
            })
            .collect();
        let _ = self.store.record_recipients(&recipients);
        self.store.outbox_push(
            account.id,
            OutboxKind::Send,
            &draft.draft_id,
            &built.raw,
            &built.from,
            &built.to,
            draft.reply_to_message,
            draft.subject.trim(),
        )?;
        let _ = self.store.delete_draft_meta(&draft.draft_id);
        Ok(())
    }

    /// Entwurf in den Postausgang legen (ersetzt frühere Fassungen).
    pub async fn queue_draft(&self, draft: &ComposeDraft) -> Result<()> {
        let account = self.store.account(draft.account_id)?;
        let files = self.read_attachments(&account, draft).await?;
        let document = self.render_document(&account, draft)?;
        let built = compose::build(&account, draft, &document, files, true)?;
        self.store.set_draft_meta(
            &draft.draft_id,
            &DraftMeta {
                template_id: draft.template_id,
                signature_id: draft.signature_id,
                editor_html: draft.html.clone(),
                code: draft.code.clone(),
            },
        )?;
        self.store.outbox_push(
            account.id,
            OutboxKind::Draft,
            &draft.draft_id,
            &built.raw,
            "",
            &[],
            None,
            draft.subject.trim(),
        )?;
        Ok(())
    }
}

// ---- Postausgang abarbeiten -------------------------------------------------

/// Ergebnis der Abarbeitung.
pub enum OutboxResult {
    Done { changed: bool },
    Offline { attempts: u32 },
}

/// Ordner einer Rolle; fehlt er, wird `fallback` benutzt und angelegt.
fn folder_path(
    store: &Store,
    account: AccountId,
    role: FolderRole,
    fallback: &str,
) -> Result<(String, bool)> {
    Ok(match store.folder_by_role(account, role)? {
        Some(f) => (f.path, false),
        None => (fallback.to_owned(), true),
    })
}

async fn imap_step(
    pool: &SessionPool,
    account: &Account,
    password: &str,
    item: &OutboxItem,
    target: (String, bool),
    drafts: (String, bool),
) -> Result<()> {
    let item = item.clone();
    pool.run(account, password, move |s| {
        let item = item.clone();
        let (target_path, create_target) = target.clone();
        let (drafts_path, drafts_missing) = drafts.clone();
        Box::pin(async move {
            // Alte Entwurfsfassungen löschen (gibt es den Ordner nicht, gibt es keine).
            if !drafts_missing {
                let old = s
                    .uids_with_message_id(&drafts_path, &item.message_id)
                    .await?;
                s.delete_permanently(&drafts_path, &old).await?;
            }
            match item.kind {
                OutboxKind::Sent => {
                    if create_target {
                        s.create_folder(&target_path).await?;
                    }
                    s.append(&target_path, "(\\Seen)", &item.raw).await
                }
                OutboxKind::Draft => {
                    if create_target {
                        s.create_folder(&target_path).await?;
                    }
                    s.append(&target_path, "(\\Seen \\Draft)", &item.raw).await
                }
                OutboxKind::Discard | OutboxKind::Send => Ok(()),
            }
        })
    })
    .await
}

/// Überträgt den Postausgang eines Kontos. `on_error(betreff, fehler)` meldet
/// endgültige Fehler (z. B. Empfänger abgelehnt).
pub async fn drain_outbox(
    store: &Store,
    pool: &SessionPool,
    account: &Account,
    password: &str,
    mut on_error: impl FnMut(&str, Error),
) -> Result<OutboxResult> {
    let mut changed = false;
    while let Some(item) = store.outbox_next(account.id)? {
        let result = match item.kind {
            OutboxKind::Send => {
                kolibri_mail::smtp::send_raw(
                    &account.smtp,
                    password,
                    &item.envelope_from,
                    &item.envelope_to,
                    &item.raw,
                )
                .await
            }
            OutboxKind::Sent => {
                let sent = folder_path(store, account.id, FolderRole::Sent, SENT_NAME)?;
                let drafts = folder_path(store, account.id, FolderRole::Drafts, DRAFTS_NAME)?;
                imap_step(pool, account, password, &item, sent, drafts).await
            }
            OutboxKind::Draft | OutboxKind::Discard => {
                let drafts = folder_path(store, account.id, FolderRole::Drafts, DRAFTS_NAME)?;
                imap_step(pool, account, password, &item, drafts.clone(), drafts).await
            }
        };
        match result {
            Ok(()) => {
                changed = true;
                match item.kind {
                    // Gesendet → im nächsten Schritt Kopie ablegen.
                    OutboxKind::Send => store.outbox_set_kind(item.id, OutboxKind::Sent)?,
                    OutboxKind::Sent => {
                        if let Some(answered) = item.answered_id {
                            for r in store.message_refs(&[answered])? {
                                store.enqueue(
                                    r.account_id,
                                    r.folder_id,
                                    &[r.uid],
                                    &Action::SetAnswered { value: true },
                                )?;
                            }
                        }
                        store.outbox_done(item.id)?;
                    }
                    OutboxKind::Draft | OutboxKind::Discard => store.outbox_done(item.id)?,
                }
            }
            Err(e) if is_connection_error(&e) || e.kind == ErrorKind::Auth => {
                store.outbox_retry(item.id, &e.to_string())?;
                return Ok(OutboxResult::Offline {
                    attempts: item.attempts + 1,
                });
            }
            Err(e) => {
                log::warn!("Postausgang ({:?}) abgelehnt: {e}", item.kind);
                if item.kind == OutboxKind::Send {
                    // Nichts verlieren: als Entwurf ablegen.
                    store.outbox_set_kind(item.id, OutboxKind::Draft)?;
                } else {
                    store.outbox_fail(item.id, &e.to_string())?;
                }
                on_error(&item.subject, e);
            }
        }
    }
    Ok(OutboxResult::Done { changed })
}

// ---- Anhänge öffnen ----------------------------------------------------------

/// Endungen, die nie per Doppelklick gestartet werden (nur Vorschau/Sichern).
const BLOCKED_OPEN: &[&str] = &[
    "app",
    "command",
    "sh",
    "zsh",
    "bash",
    "pkg",
    "mpkg",
    "scpt",
    "applescript",
    "workflow",
    "terminal",
    "jar",
    "py",
    "pl",
    "rb",
    "dylib",
    "kext",
    "action",
    "osax",
    "prefpane",
    "url",
    "webloc",
    "inetloc",
    "fileloc",
    // Windows
    "exe",
    "com",
    "bat",
    "cmd",
    "msi",
    "msix",
    "appx",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "scr",
    "pif",
    "cpl",
    "lnk",
    "reg",
    "dll",
    "sys",
    "inf",
];

#[cfg(target_os = "windows")]
fn quarantine(path: &Path) {
    // „Aus dem Internet“ (Zone 3): Windows warnt dann wie bei Browser-Downloads.
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let _ = std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
}

#[cfg(not(target_os = "windows"))]
fn quarantine(path: &Path) {
    // Gatekeeper prüft so Dateien aus Mails wie Downloads aus dem Browser.
    let stamp = format!(
        "0083;{:x};Kolibri;",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    );
    let _ = std::process::Command::new("/usr/bin/xattr")
        .args(["-w", "com.apple.quarantine", &stamp])
        .arg(path)
        .status();
}

fn attachments_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_cache_dir()
        .map(|d| d.join("attachments"))
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
}

/// Zwischenablage für geöffnete Anhänge leeren (beim Start).
pub fn clear_attachment_cache(app: &AppHandle) {
    if let Ok(dir) = attachments_dir(app) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenMode {
    QuickLook,
    Open,
}

async fn extract(
    app: &AppHandle,
    message_id: MessageId,
    index: usize,
) -> Result<(String, Vec<u8>)> {
    let state = app.state::<AppState>();
    let raw = state.fetch_raw(message_id).await?;
    let (name, _, data) = attachment_data(&raw, index)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "Anhang nicht gefunden"))?;
    Ok((safe_file_name(&name), data))
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::new(ErrorKind::Storage, e.to_string()))?;
    }
    std::fs::write(path, data).map_err(|e| Error::new(ErrorKind::Storage, e.to_string()))?;
    quarantine(path);
    Ok(())
}

// ---- Tauri-Commands ----------------------------------------------------------

#[tauri::command]
pub async fn prepare_compose(
    state: State<'_, AppState>,
    kind: ComposeKind,
) -> Result<ComposeDraft> {
    state.prepare_compose(kind).await
}

#[tauri::command]
pub async fn send_message(app: AppHandle, draft: ComposeDraft) -> Result<()> {
    let state = app.state::<AppState>();
    state.queue_send(&draft).await?;
    crate::queue::kick(app.clone(), draft.account_id);
    Ok(())
}

#[tauri::command]
pub async fn save_draft(app: AppHandle, draft: ComposeDraft) -> Result<()> {
    let state = app.state::<AppState>();
    state.queue_draft(&draft).await?;
    crate::queue::kick(app.clone(), draft.account_id);
    Ok(())
}

/// Entwurf verwerfen: auch frühere Fassungen auf dem Server löschen.
#[tauri::command]
pub fn discard_draft(app: AppHandle, account_id: AccountId, draft_id: String) -> Result<()> {
    let state = app.state::<AppState>();
    state.store.outbox_push(
        account_id,
        OutboxKind::Discard,
        &draft_id,
        &[],
        "",
        &[],
        None,
        "",
    )?;
    let _ = state.store.delete_draft_meta(&draft_id);
    crate::queue::kick(app.clone(), account_id);
    Ok(())
}

/// Vorschau der fertigen Mail (wie sie verschickt wird).
#[tauri::command]
pub fn render_preview(state: State<'_, AppState>, draft: ComposeDraft) -> Result<String> {
    let account = state.store.account(draft.account_id)?;
    state.render_document(&account, &draft)
}

// ---- Signaturen und Vorlagen --------------------------------------------------

/// Größte erlaubte Signatur (inkl. eingebetteter Bilder).
const MAX_SIGNATURE_BYTES: usize = 512 * 1024;
/// Größte erlaubte Vorlage (inkl. eingebetteter Bilder).
const MAX_TEMPLATE_BYTES: usize = 2 * 1024 * 1024;

#[tauri::command]
pub fn list_signatures(state: State<'_, AppState>) -> Result<Vec<Signature>> {
    state.store.signatures()
}

#[tauri::command]
pub fn save_signature(state: State<'_, AppState>, signature: Signature) -> Result<Signature> {
    if signature.html.len() > MAX_SIGNATURE_BYTES {
        return Err(Error::new(
            ErrorKind::Invalid,
            "Die Signatur ist zu groß (höchstens 500 KB) – bitte ein kleineres Bild verwenden.",
        ));
    }
    let clean = Signature {
        html: kolibri_mail::sanitize::clean_fragment(&signature.html),
        ..signature
    };
    state.store.save_signature(&clean)
}

#[tauri::command]
pub fn delete_signature(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.delete_signature(id)
}

#[tauri::command]
pub fn set_default_signature(
    state: State<'_, AppState>,
    account_id: AccountId,
    id: Option<i64>,
) -> Result<()> {
    state.store.set_default_signature(account_id, id)
}

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> Result<Vec<Template>> {
    state.store.templates()
}

#[tauri::command]
pub fn save_template(state: State<'_, AppState>, template: Template) -> Result<Template> {
    if template.html.len() > MAX_TEMPLATE_BYTES {
        return Err(Error::new(
            ErrorKind::Invalid,
            "Die Vorlage ist zu groß (höchstens 2 MB).",
        ));
    }
    state.store.save_template(&template)
}

#[tauri::command]
pub fn delete_template(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.delete_template(id)
}

/// Name und Größe gewählter Dateien.
#[tauri::command]
pub fn file_infos(paths: Vec<String>) -> Result<Vec<ComposeAttachment>> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .map(|path| {
            let meta = std::fs::metadata(&path)
                .map_err(|e| Error::new(ErrorKind::NotFound, format!("{path}: {e}")))?;
            if !meta.is_file() {
                return Err(Error::new(
                    ErrorKind::Invalid,
                    "Ordner können nicht angehängt werden",
                ));
            }
            let name = Path::new(&path)
                .file_name()
                .map_or_else(|| "Anhang".to_owned(), |n| n.to_string_lossy().into_owned());
            Ok(ComposeAttachment::File {
                path,
                name,
                size: meta.len(),
            })
        })
        .collect()
}

/// Anhang ansehen (Quick Look) oder mit der Standard-App öffnen.
#[tauri::command]
pub async fn open_attachment(
    app: AppHandle,
    message_id: MessageId,
    index: usize,
    mode: OpenMode,
) -> Result<()> {
    let (name, data) = extract(&app, message_id, index).await?;
    let path = attachments_dir(&app)?
        .join(message_id.to_string())
        .join(&name);
    write_file(&path, &data)?;
    let ext = Path::new(&name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if matches!(mode, OpenMode::Open) && BLOCKED_OPEN.contains(&ext.as_str()) {
        return Err(Error::new(
            ErrorKind::Invalid,
            "Dieser Dateityp kann Programme ausführen und wird aus Sicherheitsgründen nicht direkt geöffnet. Du kannst ihn sichern.",
        ));
    }
    show_file(&app, &path, mode)
}

/// macOS: Quick Look (`qlmanage -p`) bzw. `open`.
#[cfg(target_os = "macos")]
fn show_file(_app: &AppHandle, path: &Path, mode: OpenMode) -> Result<()> {
    let mut cmd = match mode {
        OpenMode::QuickLook => {
            let mut c = std::process::Command::new("/usr/bin/qlmanage");
            c.arg("-p");
            c
        }
        OpenMode::Open => std::process::Command::new("/usr/bin/open"),
    };
    cmd.arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
    Ok(())
}

/// Windows/Linux: keine Schnellansicht – mit dem Standardprogramm öffnen.
/// (Gesperrte Typen werden auch für die Vorschau nicht geöffnet.)
#[cfg(not(target_os = "macos"))]
fn show_file(app: &AppHandle, path: &Path, _mode: OpenMode) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if BLOCKED_OPEN.contains(&ext.as_str()) {
        return Err(Error::new(
            ErrorKind::Invalid,
            "Dieser Dateityp kann Programme ausführen und wird aus Sicherheitsgründen nicht direkt geöffnet. Du kannst ihn sichern.",
        ));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
}

/// Anhang an einem vom Nutzer gewählten Ort sichern.
#[tauri::command]
pub async fn save_attachment(
    app: AppHandle,
    message_id: MessageId,
    index: usize,
    path: String,
) -> Result<()> {
    let (_, data) = extract(&app, message_id, index).await?;
    write_file(Path::new(&path), &data)
}

/// Gegen den lokalen Testserver: `scripts/test-all.sh` (oder einzeln mit
/// `KOLIBRI_TEST_SERVER=1 cargo test -p kolibri --lib -- --ignored compose`).
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod server_tests {
    use super::*;
    use crate::sync::{sync_folder, sync_order};
    use kolibri_core::{MailProvider, MessageScope, MessageSummary, Security, ServerConfig};
    use kolibri_mail::ImapSession;
    use kolibri_store::NewAccount;

    const PASSWORD: &str = "kolibri-test";

    async fn resync(store: &Store, account: AccountId) {
        let cfg = store.account(account).unwrap().imap;
        let mut s = ImapSession::connect(&cfg, PASSWORD).await.unwrap();
        store
            .replace_folders(account, &s.list_folders().await.unwrap())
            .unwrap();
        for f in sync_order(&store.folders(account).unwrap()) {
            sync_folder(&mut s, store, &f).await.unwrap();
        }
        let _ = s.logout().await;
    }

    fn in_role(
        store: &Store,
        account: AccountId,
        role: FolderRole,
        subject: &str,
    ) -> Vec<MessageSummary> {
        let Some(f) = store.folder_by_role(account, role).unwrap() else {
            return vec![];
        };
        store
            .list_messages(MessageScope::Folder { id: f.id }, 0, 10_000)
            .unwrap()
            .into_iter()
            .filter(|m| m.subject == subject)
            .collect()
    }

    async fn drain(store: &Store, pool: &SessionPool, account: &Account) {
        let r = drain_outbox(store, pool, account, PASSWORD, |s, e| panic!("{s}: {e}"))
            .await
            .unwrap();
        assert!(matches!(r, OutboxResult::Done { .. }));
        let ops = crate::queue::drain_ops(store, pool, account, PASSWORD, |_, e| panic!("{e}"))
            .await
            .unwrap();
        assert!(matches!(ops, crate::queue::Drained::Done { .. }));
    }

    #[tokio::test]
    #[ignore = "sendet Mails auf dem Testserver"]
    async fn send_reply_and_drafts_against_server() {
        if std::env::var("KOLIBRI_TEST_SERVER").is_err() {
            return;
        }
        let cfg = |port| ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::None,
            username: "test@kolibri.test".into(),
        };
        let store = Store::open_in_memory().unwrap();
        let account = store
            .insert_account(&NewAccount {
                email: "test@kolibri.test".into(),
                display_name: "Test Kolibri".into(),
                imap: cfg(1143),
                smtp: cfg(1587),
            })
            .unwrap();
        resync(&store, account.id).await;
        let pool = SessionPool::default();
        let stamp = new_message_id("x@y");

        // 1. Entwurf speichern, dann zweite Fassung: nur eine bleibt.
        let draft_id = new_message_id(&account.email);
        let subject = format!("Kolibri-Test {stamp}");
        let mut draft = ComposeDraft {
            account_id: account.id,
            draft_id: draft_id.clone(),
            to: "test@kolibri.test".into(),
            cc: String::new(),
            bcc: String::new(),
            subject: subject.clone(),
            html: "<p>Erste Fassung</p>".into(),
            attachments: vec![],
            in_reply_to: None,
            references: None,
            reply_to_message: None,
            signature_id: None,
            template_id: None,
            code: None,
        };
        for html in [
            "<p>Erste Fassung</p>",
            "<p>Zweite Fassung mit <b>Anhang</b></p>",
        ] {
            draft.html = html.into();
            let built =
                compose::build(&account, &draft, &wrap_html(&draft.html), vec![], true).unwrap();
            store
                .outbox_push(
                    account.id,
                    OutboxKind::Draft,
                    &draft_id,
                    &built.raw,
                    "",
                    &[],
                    None,
                    &subject,
                )
                .unwrap();
            drain(&store, &pool, &account).await;
        }
        resync(&store, account.id).await;
        let drafts = in_role(&store, account.id, FolderRole::Drafts, &subject);
        assert_eq!(drafts.len(), 1, "genau ein Entwurf erwartet");

        // 2. Senden (mit Anhang) an sich selbst: Entwurf weg, Kopie in Gesendet, Mail im Posteingang.
        let files = vec![FilePart {
            name: "Notiz.txt".into(),
            data: b"Hallo Anhang".to_vec(),
        }];
        let built =
            compose::build(&account, &draft, &wrap_html(&draft.html), files, false).unwrap();
        store
            .outbox_push(
                account.id,
                OutboxKind::Send,
                &draft_id,
                &built.raw,
                &built.from,
                &built.to,
                None,
                &subject,
            )
            .unwrap();
        drain(&store, &pool, &account).await;
        // Zustellung an das eigene Postfach kann einen Moment dauern.
        let mut inbox = vec![];
        for _ in 0..20 {
            resync(&store, account.id).await;
            inbox = in_role(&store, account.id, FolderRole::Inbox, &subject);
            if !inbox.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert_eq!(inbox.len(), 1, "Mail nicht angekommen");
        assert!(inbox[0].has_attachments);
        assert_eq!(
            in_role(&store, account.id, FolderRole::Sent, &subject).len(),
            1,
            "keine Kopie in Gesendet"
        );
        assert!(
            in_role(&store, account.id, FolderRole::Drafts, &subject).is_empty(),
            "Entwurf nicht gelöscht"
        );

        // 3. Antwort: Original wird als beantwortet markiert, Threading-Kopfzeilen stimmen.
        let original = inbox[0].id;
        let src = store.compose_source(original).unwrap();
        let reply = ComposeDraft {
            draft_id: new_message_id(&account.email),
            subject: prefixed_subject("Re", &subject),
            in_reply_to: src
                .message_id
                .as_ref()
                .map(|m| format!("<{}>", strip_brackets(m))),
            references: src
                .message_id
                .as_ref()
                .map(|m| reply_references(None, None, m)),
            reply_to_message: Some(original),
            signature_id: None,
            template_id: None,
            code: None,
            html: "<p>Antwort</p>".into(),
            ..draft.clone()
        };
        let built =
            compose::build(&account, &reply, &wrap_html(&reply.html), vec![], false).unwrap();
        store
            .outbox_push(
                account.id,
                OutboxKind::Send,
                &reply.draft_id,
                &built.raw,
                &built.from,
                &built.to,
                Some(original),
                &reply.subject,
            )
            .unwrap();
        drain(&store, &pool, &account).await;
        resync(&store, account.id).await;
        let answered = in_role(&store, account.id, FolderRole::Inbox, &subject);
        assert!(
            answered[0].answered,
            "Original nicht als beantwortet markiert"
        );
        assert_eq!(store.queue_counts(account.id).unwrap().pending, 0);

        // 4. Server lehnt ab (fremder Absender) → Nachricht landet in den Entwürfen.
        let bad_id = new_message_id(&account.email);
        let bad_subject = format!("Abgelehnt {stamp}");
        let bad = ComposeDraft {
            draft_id: bad_id.clone(),
            subject: bad_subject.clone(),
            ..draft.clone()
        };
        let built = compose::build(&account, &bad, &wrap_html(&bad.html), vec![], false).unwrap();
        store
            .outbox_push(
                account.id,
                OutboxKind::Send,
                &bad_id,
                &built.raw,
                "fremd@andere.example",
                &built.to,
                None,
                &bad_subject,
            )
            .unwrap();
        let mut errors = vec![];
        let r = drain_outbox(&store, &pool, &account, PASSWORD, |s, e| {
            errors.push((s.to_owned(), e));
        })
        .await
        .unwrap();
        println!("Ablehnung: {errors:?}");
        assert!(matches!(r, OutboxResult::Done { .. }));
        assert_eq!(errors.len(), 1, "Ablehnung nicht gemeldet");
        assert_eq!(errors[0].0, bad_subject);
        resync(&store, account.id).await;
        assert_eq!(
            in_role(&store, account.id, FolderRole::Drafts, &bad_subject).len(),
            1,
            "abgelehnte Mail nicht in den Entwürfen"
        );
    }
}
