//! watchword's [`Store`] over the `auth_*` tables of mumbler's database.
//!
//! It shares the database's one connection, so it works the same for a file
//! and for `--memory`. Every write runs in one transaction. Foreign keys are
//! not enforced on this connection, so deleting a user deletes its sessions
//! and links here.

use std::error::Error as StdError;
use std::fmt;

use auth::store::{self, Credential, Session, Store, StoreError, Token, TokenKind, User};
use auth::{Access, Conflict, Method, NewUser, Role, UserChange, UserId};
use jiff::Timestamp;
use sqll::{Bind, Connection, Row};

use crate::Database;

const USER: &str = "SELECT id, login, email, role, access, method, password_hash, google_sub, created_at FROM auth_users";

/// The auth store. Clones share the database.
#[derive(Clone)]
pub(crate) struct AuthStore {
    db: Database,
}

impl AuthStore {
    pub(crate) fn new(db: Database) -> Self {
        Self { db }
    }

    async fn read<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, Error> + Send + 'static,
    ) -> store::Result<T>
    where
        T: Send + 'static,
    {
        let result = self.db.with_connection(f).await.map_err(StoreError::new)?;
        Ok(result?)
    }

    async fn write<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, Error> + Send + 'static,
    ) -> store::Result<T>
    where
        T: Send + 'static,
    {
        let result = self
            .db
            .with_connection(move |c| c.transaction(f))
            .await
            .map_err(StoreError::new)?;

        Ok(result?)
    }
}

fn user_where(c: &Connection, column: &str, bind: impl Bind) -> Result<Option<User>, Error> {
    let row = c
        .prepare(format!("{USER} WHERE {column} = ?"))?
        .first::<AccountRow>(bind)?;
    row.map(User::try_from).transpose()
}

fn user(c: &Connection, id: UserId) -> Result<Option<User>, Error> {
    user_where(c, "id", id)
}

/// The conflict, if `login` or `email` belongs to a user other than `id`.
fn conflict(
    c: &Connection,
    id: Option<UserId>,
    login: Option<&str>,
    email: Option<&str>,
) -> Result<Option<Conflict>, Error> {
    if let Some(login) = login
        && let Some(user) = user_where(c, "login", login)?
        && Some(user.id) != id
    {
        return Ok(Some(Conflict::Login));
    }

    if let Some(email) = email
        && let Some(user) = user_where(c, "email", email)?
        && Some(user.id) != id
    {
        return Ok(Some(Conflict::Email));
    }

    Ok(None)
}

fn link(c: &Connection, id: &str) -> Result<Option<Token>, Error> {
    let row = c
        .prepare("SELECT id, user_id, kind, expires_at, used_at FROM auth_links WHERE id = ?")?
        .first::<LinkRow>(id)?;
    row.map(Token::try_from).transpose()
}

/// Sets the credential and activates the user; whether the user exists.
fn set_credential(c: &Connection, id: UserId, cred: &Credential) -> Result<bool, Error> {
    let changed = match cred {
        Credential::Password(hash) => c
            .prepare(
                "UPDATE auth_users SET method = 'password', password_hash = ?, google_sub = NULL, \
                 access = 'active' WHERE id = ?",
            )?
            .execute((hash.as_str(), id))?,
        Credential::Google { sub, email } => c
            .prepare(
                "UPDATE auth_users SET method = 'google', password_hash = NULL, google_sub = ?, \
                 email = ?, access = 'active' WHERE id = ?",
            )?
            .execute((sub.as_str(), email.as_str(), id))?,
    };

    Ok(changed > 0)
}

fn delete_sessions(c: &Connection, user: UserId, keep: Option<&str>) -> Result<(), Error> {
    c.prepare("DELETE FROM auth_sessions WHERE user_id = ? AND id IS NOT ?")?
        .execute((user, keep))?;
    Ok(())
}

fn delete_expired_sessions(c: &Connection, now: Timestamp) -> Result<(), Error> {
    c.prepare("DELETE FROM auth_sessions WHERE expires_at <= ?")?
        .execute(millis(now))?;
    Ok(())
}

fn insert_session(c: &Connection, session: &Session) -> Result<(), Error> {
    c.prepare(
        "INSERT INTO auth_sessions (id, user_id, impersonator_id, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?)",
    )?
    .execute((
        session.id.as_str(),
        session.user,
        session.impersonator,
        millis(session.created_at),
        millis(session.expires_at),
    ))?;
    Ok(())
}

fn use_up_links(c: &Connection, user: UserId, now: Timestamp) -> Result<(), Error> {
    c.prepare("UPDATE auth_links SET used_at = ? WHERE user_id = ? AND used_at IS NULL")?
        .execute((millis(now), user))?;
    Ok(())
}

impl Store for AuthStore {
    async fn user(&self, id: UserId) -> store::Result<Option<User>> {
        self.read(move |c| user(c, id)).await
    }

    async fn user_by_email(&self, email: &str) -> store::Result<Option<User>> {
        let email = email.to_owned();
        self.read(move |c| user_where(c, "email", email.as_str()))
            .await
    }

    async fn user_by_login(&self, login: &str) -> store::Result<Option<User>> {
        let login = login.to_owned();
        self.read(move |c| user_where(c, "login", login.as_str()))
            .await
    }

    async fn user_by_google_sub(&self, sub: &str) -> store::Result<Option<User>> {
        let sub = sub.to_owned();
        self.read(move |c| user_where(c, "google_sub", sub.as_str()))
            .await
    }

    async fn users(&self, now: Timestamp) -> store::Result<Vec<(User, Option<Timestamp>)>> {
        self.read(move |c| {
            let mut stmt = c.prepare(
                "SELECT u.id, u.login, u.email, u.role, u.access, u.method, u.password_hash, \
                 u.google_sub, u.created_at, \
                 (SELECT MAX(l.expires_at) FROM auth_links l \
                  WHERE l.user_id = u.id AND l.used_at IS NULL AND l.expires_at > ?) \
                 FROM auth_users u ORDER BY u.id",
            )?;

            stmt.bind(millis(now))?;

            let mut users = Vec::new();

            while let Some(row) = stmt.next::<ListedRow>()? {
                let (account, link) = row.split();
                users.push((User::try_from(account)?, link.map(timestamp).transpose()?));
            }

            Ok(users)
        })
        .await
    }

    async fn user_count(&self) -> store::Result<u64> {
        self.read(|c| {
            let count = c
                .prepare("SELECT COUNT(*) FROM auth_users")?
                .first::<i64>(())?
                .unwrap_or_default();
            Ok(u64::try_from(count).unwrap_or_default())
        })
        .await
    }

    async fn create_user(
        &self,
        new: &NewUser,
        now: Timestamp,
    ) -> store::Result<Result<User, Conflict>> {
        let new = new.clone();

        self.write(move |c| {
            let (login, email) = (new.login.as_deref(), new.email.as_deref());

            if let Some(conflict) = conflict(c, None, login, email)? {
                return Ok(Err(conflict));
            }

            let id = c
                .prepare(
                    "INSERT INTO auth_users (login, email, role, access, method, created_at) \
                     VALUES (?, ?, ?, 'pending', 'password', ?) RETURNING id",
                )?
                .first::<UserId>((login, email, new.role, millis(now)))?
                .ok_or(Error::Missing("inserted user id"))?;

            let user = user(c, id)?.ok_or(Error::Missing("inserted user"))?;
            Ok(Ok(user))
        })
        .await
    }

    async fn update_user(
        &self,
        id: UserId,
        change: &UserChange,
    ) -> store::Result<Result<Option<User>, Conflict>> {
        let change = change.clone();

        self.write(move |c| {
            let login = change.login.as_ref().and_then(Option::as_deref);
            let email = change.email.as_ref().and_then(Option::as_deref);

            if let Some(conflict) = conflict(c, Some(id), login, email)? {
                return Ok(Err(conflict));
            }

            if user(c, id)?.is_none() {
                return Ok(Ok(None));
            }

            if let Some(login) = &change.login {
                c.prepare("UPDATE auth_users SET login = ? WHERE id = ?")?
                    .execute((login.as_deref(), id))?;
            }

            if let Some(email) = &change.email {
                c.prepare("UPDATE auth_users SET email = ? WHERE id = ?")?
                    .execute((email.as_deref(), id))?;
            }

            if let Some(role) = change.role {
                c.prepare("UPDATE auth_users SET role = ? WHERE id = ?")?
                    .execute((role, id))?;
            }

            Ok(Ok(user(c, id)?))
        })
        .await
    }

    async fn delete_user(&self, id: UserId) -> store::Result<bool> {
        self.write(move |c| {
            c.prepare("DELETE FROM auth_sessions WHERE user_id = ? OR impersonator_id = ?")?
                .execute((id, id))?;
            c.prepare("DELETE FROM auth_links WHERE user_id = ?")?
                .execute(id)?;
            let deleted = c
                .prepare("DELETE FROM auth_users WHERE id = ?")?
                .execute(id)?;
            Ok(deleted > 0)
        })
        .await
    }

    async fn set_credential(
        &self,
        id: UserId,
        cred: &Credential,
        keep: Option<&str>,
    ) -> store::Result<bool> {
        let cred = cred.clone();
        let keep = keep.map(str::to_owned);

        self.write(move |c| {
            let access = c
                .prepare("SELECT access FROM auth_users WHERE id = ?")?
                .first::<Access>(id)?;

            if access.is_none_or(|a| a == Access::Revoked) {
                return Ok(false);
            }

            if !set_credential(c, id, &cred)? {
                return Ok(false);
            }

            delete_sessions(c, id, keep.as_deref())?;
            Ok(true)
        })
        .await
    }

    async fn bind_google_sub(&self, id: UserId, sub: &str) -> store::Result<bool> {
        let sub = sub.to_owned();

        self.write(move |c| {
            if let Some(bound) = user_where(c, "google_sub", sub.as_str())? {
                return Ok(bound.id == id);
            }

            let Some(user) = user(c, id)? else {
                return Ok(false);
            };

            if user.google_sub.is_some() {
                return Ok(false);
            }

            c.prepare("UPDATE auth_users SET google_sub = ? WHERE id = ?")?
                .execute((sub.as_str(), id))?;
            Ok(true)
        })
        .await
    }

    async fn set_access(&self, id: UserId, access: Access) -> store::Result<bool> {
        self.write(move |c| {
            let changed = c
                .prepare("UPDATE auth_users SET access = ? WHERE id = ?")?
                .execute((access, id))?;
            Ok(changed > 0)
        })
        .await
    }

    async fn activate(&self, id: UserId) -> store::Result<bool> {
        self.write(move |c| {
            let changed = c
                .prepare(
                    "UPDATE auth_users SET access = 'active' WHERE id = ? AND access = 'pending'",
                )?
                .execute(id)?;
            Ok(changed > 0)
        })
        .await
    }

    async fn revoke(&self, id: UserId, now: Timestamp) -> store::Result<bool> {
        self.write(move |c| {
            let changed = c
                .prepare(
                    "UPDATE auth_users SET access = 'revoked', password_hash = NULL WHERE id = ?",
                )?
                .execute(id)?;

            if changed == 0 {
                return Ok(false);
            }

            use_up_links(c, id, now)?;
            delete_sessions(c, id, None)?;
            Ok(true)
        })
        .await
    }

    async fn create_session(&self, session: &Session) -> store::Result<()> {
        let session = session.clone();

        self.write(move |c| {
            delete_expired_sessions(c, session.created_at)?;
            insert_session(c, &session)
        })
        .await
    }

    async fn session(&self, id: &str) -> store::Result<Option<Session>> {
        let id = id.to_owned();

        self.read(move |c| {
            let row = c
                .prepare(
                    "SELECT id, user_id, impersonator_id, created_at, expires_at \
                     FROM auth_sessions WHERE id = ?",
                )?
                .first::<SessionRow>(id.as_str())?;
            row.map(Session::try_from).transpose()
        })
        .await
    }

    async fn delete_session(&self, id: &str) -> store::Result<()> {
        let id = id.to_owned();

        self.write(move |c| {
            c.prepare("DELETE FROM auth_sessions WHERE id = ?")?
                .execute(id.as_str())?;
            Ok(())
        })
        .await
    }

    async fn delete_sessions(&self, user: UserId, keep: Option<&str>) -> store::Result<()> {
        let keep = keep.map(str::to_owned);
        self.write(move |c| delete_sessions(c, user, keep.as_deref()))
            .await
    }

    async fn delete_expired_sessions(&self, now: Timestamp) -> store::Result<()> {
        self.write(move |c| delete_expired_sessions(c, now)).await
    }

    async fn create_token(&self, token: &Token, now: Timestamp) -> store::Result<()> {
        let token = token.clone();

        self.write(move |c| {
            use_up_links(c, token.user, now)?;
            c.prepare(
                "INSERT INTO auth_links (id, user_id, kind, expires_at, used_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )?
            .execute((
                token.id.as_str(),
                token.user,
                token.kind.as_str(),
                millis(token.expires_at),
                token.used_at.map(millis),
            ))?;
            Ok(())
        })
        .await
    }

    async fn token(&self, id: &str) -> store::Result<Option<Token>> {
        let id = id.to_owned();
        self.read(move |c| link(c, &id)).await
    }

    async fn delete_expired_tokens(&self, before: Timestamp) -> store::Result<()> {
        self.write(move |c| {
            c.prepare("DELETE FROM auth_links WHERE expires_at <= ?")?
                .execute(millis(before))?;
            Ok(())
        })
        .await
    }

    async fn use_up_tokens(&self, user: UserId, now: Timestamp) -> store::Result<()> {
        self.write(move |c| use_up_links(c, user, now)).await
    }

    async fn redeem_token(
        &self,
        token: &str,
        cred: &Credential,
        session: &Session,
        now: Timestamp,
    ) -> store::Result<bool> {
        let token = token.to_owned();
        let cred = cred.clone();
        let session = session.clone();

        self.write(move |c| {
            let Some(link) = link(c, &token)? else {
                return Ok(false);
            };

            if link.used_at.is_some() {
                return Ok(false);
            }

            if !set_credential(c, link.user, &cred)? {
                return Ok(false);
            }

            c.prepare("UPDATE auth_links SET used_at = ? WHERE id = ? AND used_at IS NULL")?
                .execute((millis(now), token.as_str()))?;
            delete_sessions(c, link.user, None)?;
            insert_session(c, &session)?;
            Ok(true)
        })
        .await
    }

    async fn config_get(&self, key: &str) -> store::Result<Option<String>> {
        let key = key.to_owned();

        self.read(move |c| {
            let value = c
                .prepare("SELECT value FROM auth_settings WHERE key = ?")?
                .first::<String>(key.as_str())?;
            Ok(value)
        })
        .await
    }

    async fn config_set(&self, key: &str, value: &str) -> store::Result<()> {
        let (key, value) = (key.to_owned(), value.to_owned());

        self.write(move |c| {
            c.prepare(
                "INSERT INTO auth_settings (key, value) VALUES (?, ?) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )?
            .execute((key.as_str(), value.as_str()))?;
            Ok(())
        })
        .await
    }
}

/// The store's errors, which reach watchword as a [`StoreError`].
#[derive(Debug)]
enum Error {
    Sql(sqll::Error),
    Time(jiff::Error),
    Value(Box<dyn StdError + Send + Sync>),
    Missing(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sql(e) => e.fmt(f),
            Error::Time(e) => e.fmt(f),
            Error::Value(e) => e.fmt(f),
            Error::Missing(what) => write!(f, "{what} is missing"),
        }
    }
}

impl StdError for Error {}

impl From<sqll::Error> for Error {
    fn from(e: sqll::Error) -> Self {
        Error::Sql(e)
    }
}

impl From<jiff::Error> for Error {
    fn from(e: jiff::Error) -> Self {
        Error::Time(e)
    }
}

impl From<Error> for StoreError {
    fn from(e: Error) -> Self {
        StoreError::new(e)
    }
}

#[derive(Row)]
struct AccountRow {
    id: UserId,
    login: Option<String>,
    email: Option<String>,
    role: Role,
    access: Access,
    method: Method,
    password_hash: Option<String>,
    google_sub: Option<String>,
    created_at: i64,
}

impl TryFrom<AccountRow> for User {
    type Error = Error;

    fn try_from(row: AccountRow) -> Result<Self, Error> {
        Ok(User {
            id: row.id,
            login: row.login,
            email: row.email,
            role: row.role,
            access: row.access,
            method: row.method,
            password_hash: row.password_hash,
            google_sub: row.google_sub,
            created_at: timestamp(row.created_at)?,
        })
    }
}

/// An [`AccountRow`] with the expiry of the user's live link.
#[derive(Row)]
struct ListedRow {
    id: UserId,
    login: Option<String>,
    email: Option<String>,
    role: Role,
    access: Access,
    method: Method,
    password_hash: Option<String>,
    google_sub: Option<String>,
    created_at: i64,
    link: Option<i64>,
}

impl ListedRow {
    fn split(self) -> (AccountRow, Option<i64>) {
        let account = AccountRow {
            id: self.id,
            login: self.login,
            email: self.email,
            role: self.role,
            access: self.access,
            method: self.method,
            password_hash: self.password_hash,
            google_sub: self.google_sub,
            created_at: self.created_at,
        };

        (account, self.link)
    }
}

#[derive(Row)]
struct SessionRow {
    id: String,
    user_id: UserId,
    impersonator_id: Option<UserId>,
    created_at: i64,
    expires_at: i64,
}

impl TryFrom<SessionRow> for Session {
    type Error = Error;

    fn try_from(row: SessionRow) -> Result<Self, Error> {
        Ok(Session {
            id: row.id,
            user: row.user_id,
            impersonator: row.impersonator_id,
            created_at: timestamp(row.created_at)?,
            expires_at: timestamp(row.expires_at)?,
        })
    }
}

#[derive(Row)]
struct LinkRow {
    id: String,
    user_id: UserId,
    kind: String,
    expires_at: i64,
    used_at: Option<i64>,
}

impl TryFrom<LinkRow> for Token {
    type Error = Error;

    fn try_from(row: LinkRow) -> Result<Self, Error> {
        Ok(Token {
            id: row.id,
            user: row.user_id,
            kind: row
                .kind
                .parse::<TokenKind>()
                .map_err(|e| Error::Value(e.into()))?,
            expires_at: timestamp(row.expires_at)?,
            used_at: row.used_at.map(timestamp).transpose()?,
        })
    }
}

fn millis(timestamp: Timestamp) -> i64 {
    timestamp.as_millisecond()
}

fn timestamp(millis: i64) -> Result<Timestamp, Error> {
    Ok(Timestamp::from_millisecond(millis)?)
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::AuthStore;
    use crate::{Database, Paths};

    fn open() -> Result<AuthStore, Box<dyn StdError>> {
        let paths = Paths::new(std::path::Path::new("unused"));
        Ok(AuthStore::new(Database::open(&paths, true)?))
    }

    auth::store_suite!(open()?);
}
