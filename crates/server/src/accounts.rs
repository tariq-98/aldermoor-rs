//! Accounts and the character database (#88): players log in with an account name and password,
//! and their characters (levels, XP, pack, coins, where they stood) are kept in a SQLite file on
//! the server, so they're still there after it restarts.
//!
//! Passwords are kept only as Argon2id hashes. A character belongs to the account that made it;
//! names are unique across the server.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{params, Connection, OptionalExtension};
use sim::CharacterRecord;
use std::path::Path;
use std::sync::Mutex;

/// Why a login was turned away, in words for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refuse<T>(why: &str) -> Result<T, Refusal> {
    Err(Refusal(why.to_string()))
}

/// Account and character names: 3 to 20 letters, digits, spaces, hyphens or apostrophes.
fn valid_name(name: &str) -> bool {
    let n = name.chars().count();
    (3..=20).contains(&n) && name.trim() == name && name.chars().all(|c| c.is_alphanumeric() || " -'".contains(c))
}

pub struct Store {
    db: Mutex<Connection>,
}

impl Store {
    /// Opens (or makes) the database at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        Store::with(Connection::open(path)?)
    }

    /// A database in memory, for tests.
    pub fn in_memory() -> rusqlite::Result<Store> {
        Store::with(Connection::open_in_memory()?)
    }

    fn with(db: Connection) -> rusqlite::Result<Store> {
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS accounts (name TEXT PRIMARY KEY COLLATE NOCASE, hash TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS characters (
                 name TEXT PRIMARY KEY COLLATE NOCASE,
                 account TEXT NOT NULL REFERENCES accounts(name),
                 record BLOB NOT NULL,
                 saved_at INTEGER NOT NULL
             );",
        )?;
        Ok(Store { db: Mutex::new(db) })
    }

    /// Makes a new account. Refused if the name is taken or not a name, or the password is
    /// shorter than 8 characters.
    pub fn create_account(&self, name: &str, password: &str) -> Result<(), Refusal> {
        if !valid_name(name) {
            return refuse("Account names are 3 to 20 letters, digits, spaces, hyphens or apostrophes.");
        }
        if password.chars().count() < 8 {
            return refuse("Passwords need at least 8 characters.");
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default().hash_password(password.as_bytes(), &salt).map_err(|e| Refusal(e.to_string()))?.to_string();
        let db = self.db.lock().unwrap();
        match db.execute("INSERT INTO accounts (name, hash) VALUES (?1, ?2)", params![name, hash]) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => refuse("That account name is taken."),
            Err(e) => Err(Refusal(e.to_string())),
        }
    }

    /// Whether `password` is `name`'s.
    pub fn check(&self, name: &str, password: &str) -> bool {
        let hash: Option<String> =
            self.db.lock().unwrap().query_row("SELECT hash FROM accounts WHERE name = ?1", params![name], |r| r.get(0)).optional().ok().flatten();
        hash.is_some_and(|h| PasswordHash::new(&h).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok()))
    }

    /// The account a character belongs to, if it exists.
    pub fn owner(&self, character: &str) -> Option<String> {
        self.db.lock().unwrap().query_row("SELECT account FROM characters WHERE name = ?1", params![character], |r| r.get(0)).optional().ok().flatten()
    }

    /// An account's characters, by name.
    pub fn characters(&self, account: &str) -> Vec<String> {
        let db = self.db.lock().unwrap();
        let Ok(mut q) = db.prepare("SELECT name FROM characters WHERE account = ?1 ORDER BY name") else { return Vec::new() };
        q.query_map(params![account], |r| r.get(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// A character as it was last saved.
    pub fn load(&self, character: &str) -> Option<CharacterRecord> {
        let blob: Vec<u8> = self.db.lock().unwrap().query_row("SELECT record FROM characters WHERE name = ?1", params![character], |r| r.get(0)).optional().ok()??;
        postcard::from_bytes(&blob).ok()
    }

    /// Keeps `record` for `account`, replacing what was saved before.
    pub fn save(&self, account: &str, record: &CharacterRecord) -> rusqlite::Result<()> {
        let blob = postcard::to_stdvec(record).expect("records always serialise");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        self.db.lock().unwrap().execute(
            "INSERT INTO characters (name, account, record, saved_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET record = excluded.record, saved_at = excluded.saved_at",
            params![record.name, account, blob, now],
        )?;
        Ok(())
    }

    /// Logs `account` in with `password` to play `character`: makes the account first when
    /// `create` is set, and checks the character is theirs (or free, for a new one).
    pub fn login(&self, account: &str, password: &str, character: &str, create: bool) -> Result<(), Refusal> {
        if create {
            self.create_account(account, password)?;
        } else if !self.check(account, password) {
            return refuse("Wrong account name or password.");
        }
        if !valid_name(character) {
            return refuse("Character names are 3 to 20 letters, digits, spaces, hyphens or apostrophes.");
        }
        match self.owner(character) {
            Some(owner) if !owner.eq_ignore_ascii_case(account) => refuse("That character name is taken."),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, coins: u32) -> CharacterRecord {
        CharacterRecord {
            name: name.into(),
            specialty: "archer".into(),
            levels: [3, 1, 1, 1, 1, 1],
            xp: [10, 0, 0, 0, 0, 0],
            unassigned: 5,
            slots: vec![Some(("wolf_pelt".into(), 2, false)), None],
            coins,
            at: [1.0, 2.0],
            yaw: 0.5,
        }
    }

    #[test]
    fn accounts_check_their_passwords_and_names_are_unique() {
        let s = Store::in_memory().unwrap();
        s.create_account("Chippy", "correct horse").unwrap();
        assert!(s.check("Chippy", "correct horse"));
        assert!(s.check("chippy", "correct horse"), "names ignore case");
        assert!(!s.check("Chippy", "wrong password"));
        assert!(!s.check("Nobody", "correct horse"));
        assert_eq!(s.create_account("CHIPPY", "another one"), Err(Refusal("That account name is taken.".into())));
        assert!(s.create_account("Al", "long enough").is_err(), "too short a name");
        assert!(s.create_account("Someone", "short").is_err(), "too short a password");
    }

    #[test]
    fn characters_belong_to_their_account_and_come_back_as_saved() {
        let s = Store::in_memory().unwrap();
        s.login("Chippy", "correct horse", "Brambleguard", true).unwrap();
        s.save("Chippy", &record("Brambleguard", 120)).unwrap();
        s.save("Chippy", &record("Brambleguard", 300)).unwrap();
        assert_eq!(s.load("Brambleguard"), Some(record("Brambleguard", 300)));
        assert_eq!(s.characters("Chippy"), vec!["Brambleguard".to_string()]);
        // Someone else can't take the name.
        s.create_account("Other", "password one").unwrap();
        assert!(s.login("Other", "password one", "brambleguard", false).is_err());
        assert!(s.login("Chippy", "wrong password", "Brambleguard", false).is_err());
        assert!(s.login("Chippy", "correct horse", "Brambleguard", false).is_ok());
    }
}
