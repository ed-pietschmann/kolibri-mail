//! Passwörter im macOS-Schlüsselbund bzw. in der Windows-Anmeldeinformationsverwaltung.
//!
//! Dienst: die Bundle-ID, Konto: `account-<id>`. Im Klartext liegt nirgends
//! ein Passwort – weder in der Datenbank noch in Logs.

use kolibri_core::{AccountId, Error, ErrorKind, Result, SecretStore};

#[cfg(not(test))]
const SERVICE: &str = "app.kolibri.mail";
/// Tests nutzen einen eigenen Dienstnamen und räumen hinterher auf.
#[cfg(test)]
const SERVICE: &str = "app.kolibri.mail.test";
pub struct Keychain;

pub fn account_key(id: AccountId) -> String {
    format!("account-{id}")
}

// ---- macOS: Schlüsselbund ------------------------------------------------------

#[cfg(target_os = "macos")]
/// `errSecItemNotFound`
const NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
fn keychain_err(err: &security_framework::base::Error) -> Error {
    if err.code() == NOT_FOUND {
        Error::new(ErrorKind::NotFound, "Kein Passwort im Schlüsselbund")
    } else {
        Error::new(ErrorKind::Keychain, err.to_string())
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn set_password(&self, key: &str, password: &str) -> Result<()> {
        security_framework::passwords::set_generic_password(SERVICE, key, password.as_bytes())
            .map_err(|e| keychain_err(&e))
    }

    fn password(&self, key: &str) -> Result<String> {
        let bytes = security_framework::passwords::get_generic_password(SERVICE, key)
            .map_err(|e| keychain_err(&e))?;
        String::from_utf8(bytes)
            .map_err(|_| Error::new(ErrorKind::Keychain, "Passwort ist kein gültiges UTF-8"))
    }

    fn delete_password(&self, key: &str) -> Result<()> {
        match security_framework::passwords::delete_generic_password(SERVICE, key) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(keychain_err(&e)),
        }
    }
}

// ---- Windows: Anmeldeinformationsverwaltung --------------------------------------

#[cfg(target_os = "windows")]
fn entry(key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).map_err(|e| Error::new(ErrorKind::Keychain, e.to_string()))
}

#[cfg(target_os = "windows")]
fn vault_err(err: &keyring::Error) -> Error {
    match err {
        keyring::Error::NoEntry => Error::new(ErrorKind::NotFound, "Kein Passwort gespeichert"),
        e => Error::new(ErrorKind::Keychain, e.to_string()),
    }
}

#[cfg(target_os = "windows")]
impl SecretStore for Keychain {
    fn set_password(&self, key: &str, password: &str) -> Result<()> {
        entry(key)?
            .set_password(password)
            .map_err(|e| vault_err(&e))
    }

    fn password(&self, key: &str) -> Result<String> {
        entry(key)?.get_password().map_err(|e| vault_err(&e))
    }

    fn delete_password(&self, key: &str) -> Result<()> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(vault_err(&e)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Schreibt in den echten Schlüsselbund – nur auf Wunsch:
    /// `cargo test -p kolibri -- --ignored keychain`
    #[test]
    #[ignore = "schreibt in den macOS-Schlüsselbund"]
    fn keychain_roundtrip() {
        let kc = Keychain;
        let key = "roundtrip";
        kc.set_password(key, "geheim-ä€").unwrap();
        assert_eq!(kc.password(key).unwrap(), "geheim-ä€");
        kc.set_password(key, "neu").unwrap();
        assert_eq!(kc.password(key).unwrap(), "neu");
        kc.delete_password(key).unwrap();
        assert_eq!(kc.password(key).unwrap_err().kind, ErrorKind::NotFound);
        // Doppeltes Löschen ist kein Fehler.
        kc.delete_password(key).unwrap();
    }
}
