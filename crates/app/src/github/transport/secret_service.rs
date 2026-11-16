//! The GitHub account in the desktop keyring on Linux, through the Secret Service
//! (`docs/development/2026-10-28-linux-github-secret-service.md`). One item in the
//! collection behind the `default` alias holds the same JSON `{login, token}` the macOS
//! item holds. A stored read never asks the keyring to unlock or create anything and
//! ends within five seconds; only Connect and Disconnect may show the keyring's own
//! prompts, and they end within two minutes or soon after a Cancel, saying when a write
//! or delete already went out. Every failure fails closed: there is no file or other
//! fallback, and no message carries the token or a keyring error's own text.
//!
//! Each call opens one fresh session (`Backend::connect`), which survives a restarted
//! keyring daemon, and drives oo7's futures with `block_on` on the caller's thread: the
//! window's operation executor, never the UI thread. The flows run against a fake
//! `Backend` in tests, so they need no bus; the opt-in `tests::hermetic` tests run them
//! against a disposable gnome-keyring on a private bus, and time a stored read there.
use super::*;
use futures::future::{Either, select};
use oo7::{Secret, dbus::Error};
use std::{
    cell::Cell,
    future::Future,
    pin::pin,
    sync::{Mutex, MutexGuard, TryLockError},
};

/// The macOS item's service and account, so `secret-tool` and other Secret Service
/// clients that key items by `service` and `username` find it.
pub(super) const SERVICE: &str = "com.gitturtle.github.oauth";
pub(super) const ACCOUNT: &str = "github.com";
pub(super) const LABEL: &str = "GitTurtle GitHub authorization (github.com)";
/// Every search and the one write use all three, so another application's item with
/// the same `service` and `username` is never read as GitTurtle's.
pub(super) const ATTRIBUTES: [(&str, &str); 3] = [
    (oo7::XDG_SCHEMA_ATTRIBUTE, SERVICE),
    ("service", SERVICE),
    ("username", ACCOUNT),
];
/// A stored read never prompts, so it answers quickly or not at all.
const READ_LIMIT: Duration = Duration::from_secs(5);
/// Connect and Disconnect may wait on the keyring's own prompts.
const PROMPT_LIMIT: Duration = Duration::from_secs(120);
/// How often the cancel watch looks at the operation's Cancel: a Cancel returns well
/// within 100 ms.
const CANCEL_POLL: Duration = Duration::from_millis(25);

const NO_KEYRING: &str = "No desktop keyring answered. Start your desktop's keyring (gnome-keyring on Omarchy), then try again.";
const LOCKED: &str = "Your desktop keyring is locked. Unlock it, or choose Connect GitHub CLI account, then retry. Stored account reads never open an unlock prompt.";
const READ_DEADLINE: &str = "Your desktop keyring did not answer within 5 seconds. Try again.";
const READ_FAILED: &str = "Your desktop keyring could not read the GitHub authorization. Try again, or choose Connect GitHub CLI account.";
const INVALID: &str = "The GitHub keyring entry is invalid. Connect the account again.";
const DUPLICATES: &str = "Your desktop keyring holds more than one GitTurtle GitHub entry. Choose Disconnect GitTurtle to remove them, then connect again.";
const BUSY: &str = "Another GitHub keyring operation is active. Complete or dismiss your keyring's prompt before trying again.";
const DISMISSED: &str = "The keyring prompt was dismissed. Nothing was stored.";
const SAVE_REFUSED: &str =
    "Your desktop keyring could not save GitHub authorization. No plaintext credential was stored.";
const SAVED_MISSING: &str = "The saved GitHub authorization was not found in your desktop keyring. Connect the account again.";
const CHANGED: &str = "The GitHub authorization in your desktop keyring changed during connection. Connect the intended account again.";
const SENT_UNCONFIRMED: &str = "Your desktop keyring did not confirm saving GitHub authorization; it may remain stored. Choose Disconnect GitTurtle to remove it, or connect again.";
const SENT_UNREAD: &str = "GitHub authorization was sent to your desktop keyring but could not be read back; it may remain stored. Choose Disconnect GitTurtle to remove it, or connect again.";
const CONNECT_DEADLINE: &str = "Your desktop keyring did not finish within 2 minutes. Close its prompt if it is still open; nothing was stored.";
const CONNECT_DEADLINE_SENT: &str = "Your desktop keyring did not finish within 2 minutes. Close its prompt if it is still open; GitHub authorization may remain stored in it.";
const CONNECT_CANCELLED: &str = "GitHub account connection cancelled. Close your desktop keyring's prompt if it is still open; nothing was stored.";
const CONNECT_CANCELLED_SENT: &str = "GitHub account connection cancelled; authorization may remain stored in your desktop keyring. Close its prompt if it is still open.";
const REMOVE_DISMISSED: &str =
    "The keyring prompt was dismissed. GitHub authorization remains in your desktop keyring.";
const REMOVE_FAILED: &str = "Your desktop keyring could not remove the GitHub authorization.";
const REMAINS: &str = "Your desktop keyring still holds GitTurtle's GitHub authorization. Choose Disconnect GitTurtle again.";
const DISCONNECT_DEADLINE: &str = "Your desktop keyring did not finish within 2 minutes. Close its prompt if it is still open; nothing was removed.";
const DISCONNECT_DEADLINE_SENT: &str = "Your desktop keyring did not finish within 2 minutes. Close its prompt if it is still open; GitHub authorization may remain in it.";
const DISCONNECT_CANCELLED: &str = "Disconnect cancelled. Close your desktop keyring's prompt if it is still open; nothing was removed.";
const DISCONNECT_CANCELLED_SENT: &str = "Disconnect cancelled; GitHub authorization may remain in your desktop keyring. Choose Disconnect GitTurtle again.";

/// One credential call at a time across every window: a second is refused at once,
/// rather than waiting behind, or opening, a second keyring prompt.
static ACCESS: Mutex<()> = Mutex::new(());
fn access() -> Result<MutexGuard<'static, ()>> {
    match ACCESS.try_lock() {
        Ok(access) => Ok(access),
        // A panic during a call (oo7 0.6.0 unwraps a prompt's reply, so a keyring daemon
        // that exits mid-prompt panics; the operation executor catches it) poisons the
        // lock as the guard unwinds, but releases it. The lock guards no data, so recover
        // it rather than refuse every later call.
        Err(TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => bail!(BUSY),
    }
}

/// The Secret Service calls the flows make. `Session` is one connection with its
/// transfer session; collections and items are handles within it.
pub(super) trait Backend {
    type Session;
    type Collection;
    type Item;
    async fn connect(&self) -> Result<Self::Session, Error>;
    /// The collection behind the `default` alias, never created.
    async fn stored_collection(
        &self,
        session: &Self::Session,
    ) -> Result<Option<Self::Collection>, Error>;
    /// The default collection, which the keyring may create through its own prompt.
    async fn default_collection(&self, session: &Self::Session) -> Result<Self::Collection, Error>;
    async fn collection_locked(&self, collection: &Self::Collection) -> Result<bool, Error>;
    /// Shows the keyring's own unlock prompt when it needs one.
    async fn unlock(&self, collection: &Self::Collection) -> Result<(), Error>;
    async fn search_items(
        &self,
        collection: &Self::Collection,
        attributes: &[(&str, &str)],
    ) -> Result<Vec<Self::Item>, Error>;
    /// Replaces an item with the same attributes.
    async fn create_item(
        &self,
        collection: &Self::Collection,
        label: &str,
        attributes: &[(&str, &str)],
        secret: Secret,
    ) -> Result<(), Error>;
    async fn item_locked(&self, item: &Self::Item) -> Result<bool, Error>;
    async fn secret(&self, item: &Self::Item) -> Result<Secret, Error>;
    async fn delete(&self, item: &Self::Item) -> Result<(), Error>;
}

/// The desktop keyring through oo7's D-Bus client, never its `Keyring` wrapper (which
/// creates the default collection on open, through a prompt) or GPUI's credential API
/// (which unlocks before every read).
pub(super) struct DesktopKeyring(());
impl DesktopKeyring {
    /// A test build refuses, before any bus call, unless a test started a private bus
    /// for it (`private_test_bus`).
    pub(super) fn new() -> Result<Self> {
        #[cfg(test)]
        ensure!(
            on_private_test_bus(),
            "A test reaches a desktop keyring only on a private bus named by {FIXTURE_BUS}"
        );
        Ok(Self(()))
    }
}
/// Checked again before each connection, since zbus reads the session bus address anew
/// for every one.
#[cfg(test)]
fn on_private_test_bus() -> bool {
    private_test_bus(
        std::env::var_os(FIXTURE_BUS).as_deref(),
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").as_deref(),
        &std::env::temp_dir(),
    )
    .is_some()
}
/// Names the private session bus a hermetic test started for its own keyring.
#[cfg(test)]
const FIXTURE_BUS: &str = "GITTURTLE_KEYRING_FIXTURE_BUS";
/// Whether the session bus is one a test started, so no test reaches the developer's own
/// keyring: the fixture variable names it exactly, as a single `unix:path=` address (with
/// at most a GUID after it) whose socket is under the system temp directory.
#[cfg(test)]
fn private_test_bus(
    fixture: Option<&std::ffi::OsStr>,
    session: Option<&std::ffi::OsStr>,
    temp: &std::path::Path,
) -> Option<()> {
    use std::{
        os::unix::fs::FileTypeExt as _,
        path::{Component, Path},
    };
    let (fixture, session) = (fixture?, session?);
    let address = session.to_str().filter(|_| fixture == session)?;
    let mut keys = address.strip_prefix("unix:path=")?.split(',');
    let path = Path::new(keys.next()?);
    if address.contains([';', '%']) || !keys.all(|key| key.starts_with("guid=")) {
        return None;
    }
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return None;
    }
    let socket = std::fs::symlink_metadata(path)
        .ok()?
        .file_type()
        .is_socket();
    let directory = std::fs::canonicalize(path.parent()?).ok()?;
    (socket && directory.starts_with(std::fs::canonicalize(temp).ok()?)).then_some(())
}
impl Backend for DesktopKeyring {
    type Session = oo7::dbus::Service;
    type Collection = oo7::dbus::Collection;
    type Item = oo7::dbus::Item;
    async fn connect(&self) -> Result<Self::Session, Error> {
        #[cfg(test)]
        if !on_private_test_bus() {
            return Err(Error::ZBus(oo7::zbus::Error::Address(format!(
                "a test connects only to a private bus named by {FIXTURE_BUS}"
            ))));
        }
        oo7::dbus::Service::new().await
    }
    async fn stored_collection(
        &self,
        session: &Self::Session,
    ) -> Result<Option<Self::Collection>, Error> {
        session
            .with_alias(oo7::dbus::Service::DEFAULT_COLLECTION)
            .await
    }
    async fn default_collection(&self, session: &Self::Session) -> Result<Self::Collection, Error> {
        session.default_collection().await
    }
    async fn collection_locked(&self, collection: &Self::Collection) -> Result<bool, Error> {
        collection.is_locked().await
    }
    async fn unlock(&self, collection: &Self::Collection) -> Result<(), Error> {
        collection.unlock(None).await
    }
    async fn search_items(
        &self,
        collection: &Self::Collection,
        attributes: &[(&str, &str)],
    ) -> Result<Vec<Self::Item>, Error> {
        collection.search_items(&attributes).await
    }
    async fn create_item(
        &self,
        collection: &Self::Collection,
        label: &str,
        attributes: &[(&str, &str)],
        secret: Secret,
    ) -> Result<(), Error> {
        collection
            .create_item(label, &attributes, secret, true, None)
            .await
            .map(drop)
    }
    async fn item_locked(&self, item: &Self::Item) -> Result<bool, Error> {
        item.is_locked().await
    }
    async fn secret(&self, item: &Self::Item) -> Result<Secret, Error> {
        item.secret().await
    }
    async fn delete(&self, item: &Self::Item) -> Result<(), Error> {
        item.delete(None).await
    }
}

/// How a keyring call failed, without its text: a message names only the condition.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Failure {
    /// No bus, no keyring owning or activating the Secret Service, or no reply.
    Unanswered,
    Locked,
    Dismissed,
    Other,
}
fn failure(error: &Error) -> Failure {
    use oo7::dbus::ServiceError;
    match error {
        Error::ZBus(error) | Error::Service(ServiceError::ZBus(error)) if unanswered(error) => {
            Failure::Unanswered
        }
        Error::Service(ServiceError::IsLocked(_)) => Failure::Locked,
        Error::Dismissed => Failure::Dismissed,
        _ => Failure::Other,
    }
}
fn unanswered(error: &oo7::zbus::Error) -> bool {
    use oo7::zbus::{DBusError as _, Error as Bus, fdo::Error as Fdo};
    match error {
        Bus::Address(_) | Bus::InputOutput(_) | Bus::Connection(..) | Bus::Handshake(_) => true,
        Bus::MethodError(name, ..) => unanswered_name(name.as_str()),
        Bus::FDO(error) => match error.as_ref() {
            Fdo::ZBus(error) => unanswered(error),
            error => unanswered_name(error.name().as_str()),
        },
        _ => false,
    }
}
fn unanswered_name(name: &str) -> bool {
    name.strip_prefix("org.freedesktop.DBus.Error.")
        .is_some_and(|name| {
            matches!(
                name,
                "ServiceUnknown"
                    | "NameHasNoOwner"
                    | "NoServer"
                    | "NoReply"
                    | "Timeout"
                    | "Disconnected"
            ) || name.starts_with("Spawn.")
        })
}
fn read_failure(error: Error) -> anyhow::Error {
    anyhow!(match failure(&error) {
        Failure::Unanswered => NO_KEYRING,
        Failure::Locked => LOCKED,
        Failure::Dismissed | Failure::Other => READ_FAILED,
    })
}
fn save_failure(error: Error) -> anyhow::Error {
    anyhow!(match failure(&error) {
        Failure::Unanswered => NO_KEYRING,
        Failure::Dismissed => DISMISSED,
        Failure::Locked | Failure::Other => SAVE_REFUSED,
    })
}
fn remove_failure(error: Error) -> anyhow::Error {
    anyhow!(match failure(&error) {
        Failure::Unanswered => NO_KEYRING,
        Failure::Dismissed => REMOVE_DISMISSED,
        Failure::Locked | Failure::Other => REMOVE_FAILED,
    })
}

/// The stored authorization, read without any prompt: a missing `default` alias or item
/// is nothing stored, and a locked collection or item is refused rather than unlocked.
async fn stored<B: Backend>(backend: &B, session: &B::Session) -> Result<Option<Credential>> {
    let Some(collection) = backend
        .stored_collection(session)
        .await
        .map_err(read_failure)?
    else {
        return Ok(None);
    };
    ensure!(
        !backend
            .collection_locked(&collection)
            .await
            .map_err(read_failure)?,
        LOCKED
    );
    let mut items = backend
        .search_items(&collection, &ATTRIBUTES)
        .await
        .map_err(read_failure)?;
    ensure!(items.len() <= 1, DUPLICATES);
    let Some(item) = items.pop() else {
        return Ok(None);
    };
    ensure!(
        !backend.item_locked(&item).await.map_err(read_failure)?,
        LOCKED
    );
    let secret = backend.secret(&item).await.map_err(read_failure)?;
    serde_json::from_slice(secret.as_bytes())
        .map(Some)
        .map_err(|_| anyhow!(INVALID))
}

enum Ended<T> {
    Done(T),
    Deadline,
    Cancelled,
}
/// Runs `work` until it ends, the deadline passes or the cancel watch fires, whichever
/// comes first; the losers are dropped, which abandons any call in flight.
async fn race<T>(
    work: impl Future<Output = T>,
    deadline: impl Future<Output = ()>,
    cancelled: impl Future<Output = ()>,
) -> Ended<T> {
    let (work, cancelled, deadline) = (pin!(work), pin!(cancelled), pin!(deadline));
    match select(work, select(cancelled, deadline)).await {
        Either::Left((done, _)) => Ended::Done(done),
        Either::Right((Either::Left(_), _)) => Ended::Cancelled,
        Either::Right((Either::Right(_), _)) => Ended::Deadline,
    }
}
/// A deadline that starts now, not when first polled.
fn deadline(limit: Duration) -> impl Future<Output = ()> {
    let timer = async_io::Timer::after(limit);
    async move {
        timer.await;
    }
}

async fn read_within<B: Backend>(
    backend: &B,
    deadline: impl Future<Output = ()>,
) -> Result<Option<Credential>> {
    let work = async {
        let session = backend.connect().await.map_err(read_failure)?;
        stored(backend, &session).await
    };
    match race(work, deadline, futures::future::pending()).await {
        Ended::Done(result) => result,
        Ended::Deadline | Ended::Cancelled => bail!(READ_DEADLINE),
    }
}

/// Ends once the operation is cancelled, looking every [`CANCEL_POLL`].
async fn cancellation(control: &OperationControl) {
    while !control.is_cancelled() {
        async_io::Timer::after(CANCEL_POLL).await;
    }
}

/// The JSON the macOS item holds, straight into a `Secret`, which zeroes it on drop. The
/// buffer fits the longest possible JSON (every byte escaped as `\u00XX`), so writing it
/// never reallocates and leaves a partial copy behind; `Secret::Text` is what
/// `Secret::text` builds, without its copy.
fn secret_text(credential: &Credential) -> Result<Secret> {
    let longest = 32 + 6 * (credential.login.len() + credential.token.len());
    let mut bytes = Vec::with_capacity(longest);
    serde_json::to_writer(&mut bytes, credential).map_err(|_| anyhow!(SAVE_REFUSED))?;
    Ok(Secret::Text(
        String::from_utf8(bytes).map_err(|_| anyhow!(SAVE_REFUSED))?,
    ))
}

/// `sent` records that the write went out, after which a stopped Connect cannot say
/// nothing was stored.
async fn save_flow<B: Backend>(
    backend: &B,
    credential: &Credential,
    control: &OperationControl,
    sent: &Cell<bool>,
) -> Result<()> {
    let session = backend.connect().await.map_err(save_failure)?;
    let collection = backend
        .default_collection(&session)
        .await
        .map_err(save_failure)?;
    if backend
        .collection_locked(&collection)
        .await
        .map_err(save_failure)?
    {
        backend.unlock(&collection).await.map_err(save_failure)?;
    }
    // Unlocked first and checked here, so a Cancel during the unlock prompt stores
    // nothing even when the person completes the prompt afterwards.
    ensure!(!control.is_cancelled(), CONNECT_CANCELLED);
    let secret = secret_text(credential)?;
    sent.set(true);
    // Once the write went out, only a dismissed prompt (which stores nothing, per the
    // specification) says nothing was stored: an unanswered call (oo7's 30 s method
    // timeout) or a refused one may still have stored it.
    backend
        .create_item(&collection, LABEL, &ATTRIBUTES, secret)
        .await
        .map_err(|error| match failure(&error) {
            Failure::Dismissed => anyhow!(DISMISSED),
            _ => anyhow!(SENT_UNCONFIRMED),
        })?;
    // Read back as Refresh will, with no prompt, before reporting success.
    let restored = stored(backend, &session)
        .await
        .map_err(|_| anyhow!(SENT_UNREAD))?
        .ok_or_else(|| anyhow!(SAVED_MISSING))?;
    ensure!(
        restored.login == credential.login && restored.token == credential.token,
        CHANGED
    );
    Ok(())
}
async fn save_within<B: Backend>(
    backend: &B,
    credential: &Credential,
    control: &OperationControl,
    deadline: impl Future<Output = ()>,
    cancelled: impl Future<Output = ()>,
) -> Result<()> {
    let sent = Cell::new(false);
    let ended = race(
        save_flow(backend, credential, control, &sent),
        deadline,
        cancelled,
    )
    .await;
    match (ended, sent.get()) {
        (Ended::Done(result), _) => result,
        (Ended::Deadline, false) => bail!(CONNECT_DEADLINE),
        (Ended::Deadline, true) => bail!(CONNECT_DEADLINE_SENT),
        (Ended::Cancelled, false) => bail!(CONNECT_CANCELLED),
        (Ended::Cancelled, true) => bail!(CONNECT_CANCELLED_SENT),
    }
}

/// `sent` records that a delete went out, after which a stopped Disconnect cannot say
/// nothing was removed.
async fn remove_flow<B: Backend>(
    backend: &B,
    control: &OperationControl,
    sent: &Cell<bool>,
) -> Result<()> {
    let session = backend.connect().await.map_err(remove_failure)?;
    let Some(collection) = backend
        .stored_collection(&session)
        .await
        .map_err(remove_failure)?
    else {
        return Ok(());
    };
    if backend
        .collection_locked(&collection)
        .await
        .map_err(remove_failure)?
    {
        backend.unlock(&collection).await.map_err(remove_failure)?;
    }
    ensure!(!control.is_cancelled(), DISCONNECT_CANCELLED);
    let items = backend
        .search_items(&collection, &ATTRIBUTES)
        .await
        .map_err(remove_failure)?;
    for item in &items {
        sent.set(true);
        backend.delete(item).await.map_err(remove_failure)?;
    }
    ensure!(
        backend
            .search_items(&collection, &ATTRIBUTES)
            .await
            .map_err(remove_failure)?
            .is_empty(),
        REMAINS
    );
    Ok(())
}
async fn remove_within<B: Backend>(
    backend: &B,
    control: &OperationControl,
    deadline: impl Future<Output = ()>,
    cancelled: impl Future<Output = ()>,
) -> Result<()> {
    let sent = Cell::new(false);
    let ended = race(remove_flow(backend, control, &sent), deadline, cancelled).await;
    match (ended, sent.get()) {
        (Ended::Done(result), _) => result,
        (Ended::Deadline, false) => bail!(DISCONNECT_DEADLINE),
        (Ended::Deadline, true) => bail!(DISCONNECT_DEADLINE_SENT),
        (Ended::Cancelled, false) => bail!(DISCONNECT_CANCELLED),
        (Ended::Cancelled, true) => bail!(DISCONNECT_CANCELLED_SENT),
    }
}

/// The stored authorization, for Refresh and opening a pull request: never a prompt,
/// and an answer within [`READ_LIMIT`].
pub(super) fn load(backend: &impl Backend) -> Result<Option<Credential>> {
    let _access = access()?;
    futures::executor::block_on(read_within(backend, deadline(READ_LIMIT)))
}
/// Connect's write: the keyring may prompt to create or unlock its default collection.
/// It ends within [`PROMPT_LIMIT`] or soon after `control` is cancelled, and succeeds
/// only once a read without any prompt returns the same account and token.
pub(super) fn save(
    backend: &impl Backend,
    credential: &Credential,
    control: &OperationControl,
) -> Result<()> {
    let _access = access()?;
    futures::executor::block_on(save_within(
        backend,
        credential,
        control,
        deadline(PROMPT_LIMIT),
        cancellation(control),
    ))
}
/// Disconnect: removes every GitTurtle item, unlocking through the keyring's prompt
/// when needed, then checks that none remains. Bounded as [`save`] is.
pub(super) fn remove(backend: &impl Backend, control: &OperationControl) -> Result<()> {
    let _access = access()?;
    futures::executor::block_on(remove_within(
        backend,
        control,
        deadline(PROMPT_LIMIT),
        cancellation(control),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::oneshot;
    use std::cell::{Cell, RefCell};

    /// The call that fails, and the error it returns.
    type Failing = (&'static str, fn() -> Error);
    /// A write the fake received: its label, attributes and secret.
    type Written = (String, Vec<(String, String)>, Secret);
    /// A keyring in memory. Each call is logged by name; `fail` makes one call return an
    /// error, and `hang` makes one call never finish after it fires `fire`.
    struct Fake {
        alias: Cell<bool>,
        locked: Cell<bool>,
        item_locked: bool,
        items: RefCell<Vec<Option<Secret>>>,
        log: RefCell<Vec<&'static str>>,
        searched: RefCell<Vec<Vec<(String, String)>>>,
        created: RefCell<Vec<Written>>,
        /// What a write stores instead of the secret it was given.
        replace_with: Option<String>,
        /// Whether a write reports success but stores nothing.
        drop_writes: bool,
        /// Whether a delete reports success but keeps the item.
        delete_keeps: bool,
        fail: Option<Failing>,
        hang: Option<&'static str>,
        fire: RefCell<Option<oneshot::Sender<()>>>,
    }
    impl Fake {
        fn new(alias: bool, items: &[&str]) -> Self {
            Self {
                alias: Cell::new(alias),
                locked: Cell::new(false),
                item_locked: false,
                items: RefCell::new(items.iter().map(|text| Some(Secret::text(text))).collect()),
                log: RefCell::default(),
                searched: RefCell::default(),
                created: RefCell::default(),
                replace_with: None,
                drop_writes: false,
                delete_keeps: false,
                fail: None,
                hang: None,
                fire: RefCell::default(),
            }
        }
        /// A future that ends once the `hang` call starts.
        fn fired(&self) -> impl Future<Output = ()> + use<> {
            let (send, receive) = oneshot::channel();
            *self.fire.borrow_mut() = Some(send);
            async move {
                let _ = receive.await;
            }
        }
        fn called(&self, operation: &str) -> bool {
            self.log.borrow().contains(&operation)
        }
        async fn enter(&self, operation: &'static str) -> Result<(), Error> {
            self.log.borrow_mut().push(operation);
            if self.hang == Some(operation) {
                if let Some(fire) = self.fire.borrow_mut().take() {
                    let _ = fire.send(());
                }
                futures::future::pending::<()>().await;
            }
            match self.fail {
                Some((name, error)) if name == operation => Err(error()),
                _ => Ok(()),
            }
        }
    }
    impl Backend for Fake {
        type Session = ();
        type Collection = ();
        type Item = usize;
        async fn connect(&self) -> Result<(), Error> {
            self.enter("connect").await
        }
        async fn stored_collection(&self, _: &()) -> Result<Option<()>, Error> {
            self.enter("stored_collection").await?;
            Ok(self.alias.get().then_some(()))
        }
        async fn default_collection(&self, _: &()) -> Result<(), Error> {
            self.enter("default_collection").await?;
            self.alias.set(true);
            Ok(())
        }
        async fn collection_locked(&self, _: &()) -> Result<bool, Error> {
            self.enter("collection_locked").await?;
            Ok(self.locked.get())
        }
        async fn unlock(&self, _: &()) -> Result<(), Error> {
            self.enter("unlock").await?;
            self.locked.set(false);
            Ok(())
        }
        async fn search_items(
            &self,
            _: &(),
            attributes: &[(&str, &str)],
        ) -> Result<Vec<usize>, Error> {
            self.enter("search_items").await?;
            self.searched.borrow_mut().push(
                attributes
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            );
            Ok((self.items.borrow().iter().enumerate())
                .filter_map(|(index, item)| item.as_ref().map(|_| index))
                .collect())
        }
        async fn create_item(
            &self,
            _: &(),
            label: &str,
            attributes: &[(&str, &str)],
            secret: Secret,
        ) -> Result<(), Error> {
            self.enter("create_item").await?;
            self.created.borrow_mut().push((
                label.to_owned(),
                attributes
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
                secret.clone(),
            ));
            if self.drop_writes {
                return Ok(());
            }
            let secret = self.replace_with.as_ref().map_or(secret, Secret::text);
            // Replaces the first matching item, as `replace = true` does.
            let mut items = self.items.borrow_mut();
            match items.iter_mut().find(|item| item.is_some()) {
                Some(item) => *item = Some(secret),
                None => items.push(Some(secret)),
            }
            Ok(())
        }
        async fn item_locked(&self, _: &usize) -> Result<bool, Error> {
            self.enter("item_locked").await?;
            Ok(self.item_locked)
        }
        async fn secret(&self, item: &usize) -> Result<Secret, Error> {
            self.enter("secret").await?;
            Ok(self.items.borrow()[*item].clone().expect("a live item"))
        }
        async fn delete(&self, item: &usize) -> Result<(), Error> {
            self.enter("delete").await?;
            if !self.delete_keeps {
                self.items.borrow_mut()[*item] = None;
            }
            Ok(())
        }
    }

    const MARKER: &str = "fixture-token-5f0c";
    fn stored_json() -> String {
        format!(r#"{{"login":"fixture-reviewer","token":"{MARKER}"}}"#)
    }
    fn read(fake: &Fake) -> Result<Option<Credential>> {
        futures::executor::block_on(read_within(fake, futures::future::pending()))
    }
    fn message(result: Result<impl std::fmt::Debug>) -> String {
        format!("{:#}", result.expect_err("the keyring call must fail"))
    }

    #[test]
    fn a_stored_read_returns_the_one_item_parsed_as_the_macos_json() {
        let fake = Fake::new(true, &[&stored_json()]);
        let credential = read(&fake).unwrap().expect("the stored authorization");
        assert_eq!(credential.login, "fixture-reviewer");
        assert_eq!(credential.token, MARKER);
        assert!(!fake.called("unlock") && !fake.called("default_collection"));
    }

    #[test]
    fn a_locked_keyring_refuses_the_stored_read_without_an_unlock_prompt() {
        let fake = Fake::new(true, &[&stored_json()]);
        fake.locked.set(true);
        assert_eq!(message(read(&fake)), LOCKED);
        assert!(!fake.called("unlock") && !fake.called("secret"));

        let fake = Fake {
            item_locked: true,
            ..Fake::new(true, &[&stored_json()])
        };
        assert_eq!(message(read(&fake)), LOCKED);
        assert!(!fake.called("unlock") && !fake.called("secret"));
    }

    #[test]
    fn a_stored_read_that_never_answers_ends_at_its_deadline() {
        let fake = Fake {
            hang: Some("secret"),
            ..Fake::new(true, &[&stored_json()])
        };
        let deadline = fake.fired();
        let result = futures::executor::block_on(read_within(&fake, deadline));
        assert_eq!(message(result), READ_DEADLINE);
        assert!(fake.called("secret"));

        // Opening the session, too: the bound covers the whole read.
        let fake = Fake {
            hang: Some("connect"),
            ..Fake::new(true, &[&stored_json()])
        };
        let deadline = fake.fired();
        let result = futures::executor::block_on(read_within(&fake, deadline));
        assert_eq!(message(result), READ_DEADLINE);
    }

    #[test]
    fn no_default_alias_is_nothing_stored_and_creates_no_collection() {
        let fake = Fake::new(false, &[]);
        assert!(read(&fake).unwrap().is_none());
        assert!(!fake.called("default_collection") && !fake.called("create_item"));
        assert!(!fake.called("unlock"));

        // An alias with no GitTurtle item is nothing stored, too.
        assert!(read(&Fake::new(true, &[])).unwrap().is_none());
    }

    #[test]
    fn two_matching_entries_are_refused_without_reading_either() {
        let fake = Fake::new(true, &[&stored_json(), &stored_json()]);
        assert_eq!(message(read(&fake)), DUPLICATES);
        assert!(!fake.called("secret"));
    }

    #[test]
    fn an_invalid_entry_asks_to_connect_again_without_echoing_it() {
        let fake = Fake::new(true, &[&format!("not json {MARKER}")]);
        assert_eq!(message(read(&fake)), INVALID);
    }

    #[test]
    fn no_bus_or_no_keyring_service_asks_to_start_the_keyring() {
        use oo7::zbus::{Error as Bus, fdo::Error as Fdo};
        let unanswered: [fn() -> Error; 6] = [
            || Error::ZBus(Bus::Address("no session bus".into())),
            || {
                Error::ZBus(Bus::InputOutput(std::sync::Arc::new(
                    std::io::ErrorKind::NotFound.into(),
                )))
            },
            || Error::ZBus(Bus::FDO(Box::new(Fdo::ServiceUnknown("none".into())))),
            || Error::ZBus(Bus::FDO(Box::new(Fdo::SpawnServiceNotFound("none".into())))),
            || {
                Error::Service(oo7::dbus::ServiceError::ZBus(method_error(
                    "org.freedesktop.DBus.Error.ServiceUnknown",
                )))
            },
            || Error::ZBus(method_error("org.freedesktop.DBus.Error.NameHasNoOwner")),
        ];
        for error in unanswered {
            let fake = Fake {
                fail: Some(("connect", error)),
                ..Fake::new(true, &[&stored_json()])
            };
            assert_eq!(message(read(&fake)), NO_KEYRING);
        }
        // Another refusal is not mistaken for a missing keyring.
        let fake = Fake {
            fail: Some(("connect", || {
                Error::ZBus(method_error("org.freedesktop.DBus.Error.AccessDenied"))
            })),
            ..Fake::new(true, &[&stored_json()])
        };
        assert_eq!(message(read(&fake)), READ_FAILED);
    }
    fn method_error(name: &'static str) -> oo7::zbus::Error {
        let message = oo7::zbus::message::Message::method_call("/", "OpenSession")
            .unwrap()
            .build(&())
            .unwrap();
        oo7::zbus::Error::MethodError(name.try_into().unwrap(), None, message)
    }

    #[test]
    fn the_item_schema_matches_the_note_and_the_macos_item() {
        assert_eq!(SERVICE, "com.gitturtle.github.oauth");
        assert_eq!(ACCOUNT, "github.com");
        assert_eq!(LABEL, "GitTurtle GitHub authorization (github.com)");
        let fake = Fake::new(true, &[&stored_json()]);
        read(&fake).unwrap();
        assert_eq!(*fake.searched.borrow(), vec![schema()]);
    }
    /// The note's attribute table, spelled out rather than taken from `ATTRIBUTES`.
    fn schema() -> Vec<(String, String)> {
        [
            ("xdg:schema", "com.gitturtle.github.oauth"),
            ("service", "com.gitturtle.github.oauth"),
            ("username", "github.com"),
        ]
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .to_vec()
    }

    fn credential() -> Credential {
        Credential {
            login: "fixture-reviewer".into(),
            token: MARKER.into(),
        }
    }
    fn connect(fake: &Fake, control: &OperationControl) -> Result<()> {
        let (deadline, cancelled) = (futures::future::pending(), futures::future::pending());
        futures::executor::block_on(save_within(
            fake,
            &credential(),
            control,
            deadline,
            cancelled,
        ))
    }
    fn disconnect(fake: &Fake, control: &OperationControl) -> Result<()> {
        let (deadline, cancelled) = (futures::future::pending(), futures::future::pending());
        futures::executor::block_on(remove_within(fake, control, deadline, cancelled))
    }
    fn position(fake: &Fake, operation: &str) -> usize {
        let log = fake.log.borrow();
        log.iter().position(|logged| *logged == operation).unwrap()
    }

    #[test]
    fn connect_unlocks_first_writes_the_schema_and_reads_the_item_back() {
        let fake = Fake::new(true, &[]);
        fake.locked.set(true);
        connect(&fake, &OperationControl::default()).unwrap();
        assert!(position(&fake, "unlock") < position(&fake, "create_item"));
        assert!(position(&fake, "create_item") < position(&fake, "secret"));
        let created = fake.created.borrow();
        let [(label, attributes, secret)] = created.as_slice() else {
            panic!("Connect writes one item");
        };
        assert_eq!(label, "GitTurtle GitHub authorization (github.com)");
        assert_eq!(*attributes, schema());
        assert_eq!(secret.content_type(), oo7::ContentType::Text);
        assert_eq!(secret.as_bytes(), stored_json().as_bytes());
    }

    #[test]
    fn a_dismissed_keyring_prompt_stores_nothing() {
        let fake = Fake {
            fail: Some(("unlock", || Error::Dismissed)),
            ..Fake::new(true, &[])
        };
        fake.locked.set(true);
        assert_eq!(
            message(connect(&fake, &OperationControl::default())),
            DISMISSED
        );
        assert!(!fake.called("create_item"));

        // The prompt that creates a missing default collection, too.
        let fake = Fake {
            fail: Some(("default_collection", || Error::Dismissed)),
            ..Fake::new(false, &[])
        };
        assert_eq!(
            message(connect(&fake, &OperationControl::default())),
            DISMISSED
        );
        assert!(!fake.called("create_item"));

        // The prompt the write itself may show, for a collection locked again meanwhile.
        let fake = Fake {
            fail: Some(("create_item", || Error::Dismissed)),
            ..Fake::new(true, &[])
        };
        assert_eq!(
            message(connect(&fake, &OperationControl::default())),
            DISMISSED
        );
    }

    #[test]
    fn a_cancel_during_the_unlock_prompt_returns_and_stores_nothing() {
        let fake = Fake {
            hang: Some("unlock"),
            ..Fake::new(true, &[])
        };
        fake.locked.set(true);
        let cancelled = fake.fired();
        let result = futures::executor::block_on(save_within(
            &fake,
            &credential(),
            &OperationControl::default(),
            futures::future::pending(),
            cancelled,
        ));
        assert_eq!(message(result), CONNECT_CANCELLED);
        assert!(fake.called("unlock") && !fake.called("create_item"));
    }

    #[test]
    fn a_cancel_that_lands_as_the_prompt_completes_stores_nothing() {
        // The watch has not fired yet, but the operation is already cancelled when the
        // person completes the unlock prompt.
        let fake = Fake::new(true, &[]);
        fake.locked.set(true);
        let control = OperationControl::default();
        control.cancel();
        assert_eq!(message(connect(&fake, &control)), CONNECT_CANCELLED);
        assert!(fake.called("unlock") && !fake.called("create_item"));
    }

    #[test]
    fn the_cancel_watch_ends_only_once_the_operation_is_cancelled() {
        use futures::FutureExt as _;
        assert!(
            CANCEL_POLL <= Duration::from_millis(25),
            "Cancel returns within 100 ms"
        );
        let control = OperationControl::default();
        assert!(cancellation(&control).now_or_never().is_none());
        control.cancel();
        assert!(cancellation(&control).now_or_never().is_some());
    }

    #[test]
    fn a_stop_after_the_write_went_out_says_it_may_remain_stored() {
        let fake = Fake {
            hang: Some("create_item"),
            ..Fake::new(true, &[])
        };
        let cancelled = fake.fired();
        let control = OperationControl::default();
        let pending = futures::future::pending();
        let result = futures::executor::block_on(save_within(
            &fake,
            &credential(),
            &control,
            pending,
            cancelled,
        ));
        assert_eq!(message(result), CONNECT_CANCELLED_SENT);

        let fake = Fake {
            hang: Some("create_item"),
            ..Fake::new(true, &[])
        };
        let deadline = fake.fired();
        let pending = futures::future::pending();
        let result = futures::executor::block_on(save_within(
            &fake,
            &credential(),
            &control,
            deadline,
            pending,
        ));
        assert_eq!(message(result), CONNECT_DEADLINE_SENT);

        // The deadline during the unlock prompt, before anything was sent.
        let fake = Fake {
            hang: Some("unlock"),
            ..Fake::new(true, &[])
        };
        fake.locked.set(true);
        let deadline = fake.fired();
        let pending = futures::future::pending();
        let result = futures::executor::block_on(save_within(
            &fake,
            &credential(),
            &control,
            deadline,
            pending,
        ));
        assert_eq!(message(result), CONNECT_DEADLINE);
    }

    #[test]
    fn a_keyring_that_never_connects_ends_connect_and_disconnect_at_the_deadline() {
        let control = OperationControl::default();
        let fake = Fake {
            hang: Some("connect"),
            ..Fake::new(true, &[&stored_json()])
        };
        let (deadline, pending) = (fake.fired(), futures::future::pending());
        let result = futures::executor::block_on(save_within(
            &fake,
            &credential(),
            &control,
            deadline,
            pending,
        ));
        assert_eq!(message(result), CONNECT_DEADLINE);

        let fake = Fake {
            hang: Some("connect"),
            ..Fake::new(true, &[&stored_json()])
        };
        let (deadline, pending) = (fake.fired(), futures::future::pending());
        let result = futures::executor::block_on(remove_within(&fake, &control, deadline, pending));
        assert_eq!(message(result), DISCONNECT_DEADLINE);
    }

    #[test]
    fn a_failed_read_back_after_the_write_says_it_may_remain_stored() {
        for fake in [
            Fake {
                fail: Some(("secret", || Error::NotFound("gone".into()))),
                ..Fake::new(true, &[])
            },
            Fake {
                item_locked: true,
                ..Fake::new(true, &[])
            },
        ] {
            assert_eq!(
                message(connect(&fake, &OperationControl::default())),
                SENT_UNREAD
            );
            assert!(fake.called("create_item"));
        }
    }

    #[test]
    fn an_unanswered_write_says_it_may_remain_stored() {
        let fake = Fake {
            fail: Some(("create_item", || {
                Error::ZBus(oo7::zbus::Error::InputOutput(std::sync::Arc::new(
                    std::io::ErrorKind::TimedOut.into(),
                )))
            })),
            ..Fake::new(true, &[])
        };
        assert_eq!(
            message(connect(&fake, &OperationControl::default())),
            SENT_UNCONFIRMED
        );
    }

    #[test]
    fn a_read_back_of_another_account_or_token_fails_connect() {
        let other_token = format!(r#"{{"login":"fixture-reviewer","token":"other-{MARKER}"}}"#);
        let other_login = format!(r#"{{"login":"other-reviewer","token":"{MARKER}"}}"#);
        for stored in [other_token, other_login] {
            let fake = Fake {
                replace_with: Some(stored),
                ..Fake::new(true, &[])
            };
            assert_eq!(
                message(connect(&fake, &OperationControl::default())),
                CHANGED
            );
        }
        // A write the keyring accepted but does not return.
        let fake = Fake {
            drop_writes: true,
            ..Fake::new(true, &[])
        };
        assert_eq!(
            message(connect(&fake, &OperationControl::default())),
            SAVED_MISSING
        );
    }

    /// The only fake-keyring test that takes the process-wide lock, since a test holding it
    /// refuses every other caller; the other tests drive the flows beneath it. It holds
    /// [`serial`] throughout, as the hermetic tests that call the public functions do.
    #[test]
    fn a_second_keyring_call_is_refused_at_once_without_touching_the_keyring() {
        let _serial = serial();
        let control = OperationControl::default();
        let fake = Fake::new(true, &[&stored_json()]);
        let held = access().unwrap();
        assert_eq!(message(load(&fake)), BUSY);
        assert_eq!(message(save(&fake, &credential(), &control)), BUSY);
        assert_eq!(message(remove(&fake, &control)), BUSY);
        assert!(fake.log.borrow().is_empty());
        drop(held);
        assert_eq!(load(&fake).unwrap().unwrap().login, "fixture-reviewer");

        // A panic during a call poisons the lock as its guard unwinds; later calls run.
        std::thread::spawn(|| {
            let _access = access().unwrap();
            panic!("a keyring call panicked");
        })
        .join()
        .unwrap_err();
        assert!(ACCESS.is_poisoned());
        assert_eq!(load(&fake).unwrap().unwrap().login, "fixture-reviewer");

        // Connect watches the operation's own Cancel through the unlock prompt.
        let fake = Fake {
            hang: Some("unlock"),
            ..Fake::new(true, &[])
        };
        fake.locked.set(true);
        control.cancel();
        assert_eq!(
            message(save(&fake, &credential(), &control)),
            CONNECT_CANCELLED
        );
        assert!(!fake.called("create_item"));

        // Disconnect does, too.
        let fake = Fake {
            hang: Some("unlock"),
            ..Fake::new(true, &[&stored_json()])
        };
        fake.locked.set(true);
        assert_eq!(message(remove(&fake, &control)), DISCONNECT_CANCELLED);
        assert!(!fake.called("delete"));
    }

    #[test]
    fn disconnect_removes_every_duplicate_and_checks_none_remains() {
        let control = OperationControl::default();
        let fake = Fake::new(true, &[&stored_json(), &stored_json()]);
        fake.locked.set(true);
        disconnect(&fake, &control).unwrap();
        assert!(position(&fake, "unlock") < position(&fake, "delete"));
        let deletes = fake.log.borrow().iter().filter(|c| **c == "delete").count();
        assert_eq!(deletes, 2);
        assert!(fake.items.borrow().iter().all(Option::is_none));
        assert_eq!(fake.searched.borrow().len(), 2);

        let fake = Fake {
            delete_keeps: true,
            ..Fake::new(true, &[&stored_json()])
        };
        assert_eq!(message(disconnect(&fake, &control)), REMAINS);

        // Nothing GitTurtle saved can be in a keyring with no default collection.
        let fake = Fake::new(false, &[]);
        disconnect(&fake, &control).unwrap();
        assert!(!fake.called("unlock") && !fake.called("default_collection"));
    }

    /// Runs Connect (`connect`) or Disconnect against `fake` and stops it, by the deadline
    /// or by Cancel, once the fake's `hang` call starts.
    fn stopped(fake: &Fake, connect: bool, by_deadline: bool) -> String {
        use futures::FutureExt as _;
        let (fired, never) = (
            fake.fired().boxed_local(),
            futures::future::pending().boxed_local(),
        );
        let (deadline, cancelled) = if by_deadline {
            (fired, never)
        } else {
            (never, fired)
        };
        let control = OperationControl::default();
        message(futures::executor::block_on(async {
            match connect {
                true => save_within(fake, &credential(), &control, deadline, cancelled).await,
                false => remove_within(fake, &control, deadline, cancelled).await,
            }
        }))
    }

    #[test]
    fn no_message_carries_the_token_a_keyring_error_or_another_platforms_store() {
        let control = OperationControl::default();
        let fake = |locked: bool| {
            let fake = Fake::new(true, &[&stored_json()]);
            fake.locked.set(locked);
            fake
        };
        let hanging = |call, locked| Fake {
            hang: Some(call),
            ..fake(locked)
        };
        let failing = |call, error: fn() -> Error, locked| Fake {
            fail: Some((call, error)),
            ..fake(locked)
        };
        let detail: fn() -> Error = || Error::NotFound(format!("keyring detail {MARKER}"));
        let unanswered: fn() -> Error = || {
            Error::ZBus(oo7::zbus::Error::Address(format!(
                "keyring detail {MARKER}"
            )))
        };
        let dismissed: fn() -> Error = || Error::Dismissed;
        let mut messages = vec![
            message(read(&fake(true))),
            message(read(&Fake::new(true, &[&stored_json(), &stored_json()]))),
            message(read(&Fake::new(true, &[&format!("{{{MARKER}")]))),
            message(read(&failing("connect", unanswered, false))),
            message(read(&failing("secret", detail, false))),
            message(connect(&failing("unlock", dismissed, true), &control)),
            message(connect(&failing("unlock", detail, true), &control)),
            message(connect(&failing("create_item", detail, false), &control)),
            message(connect(&failing("secret", detail, false), &control)),
            message(connect(
                &Fake {
                    drop_writes: true,
                    ..Fake::new(true, &[])
                },
                &control,
            )),
            message(connect(
                &Fake {
                    replace_with: Some(format!(r#"{{"login":"x","token":"{MARKER}-2"}}"#)),
                    ..fake(false)
                },
                &control,
            )),
            message(disconnect(&failing("unlock", dismissed, true), &control)),
            message(disconnect(&failing("delete", detail, false), &control)),
            message(disconnect(
                &Fake {
                    delete_keeps: true,
                    ..fake(false)
                },
                &control,
            )),
        ];
        let reading = hanging("secret", false);
        let deadline = reading.fired();
        messages.push(message(futures::executor::block_on(read_within(
            &reading, deadline,
        ))));
        for connect in [true, false] {
            for by_deadline in [true, false] {
                messages.push(stopped(&hanging("unlock", true), connect, by_deadline));
                let write = if connect { "create_item" } else { "delete" };
                messages.push(stopped(&hanging(write, false), connect, by_deadline));
            }
        }
        // A second caller gets this one; the test that holds the lock checks it does.
        messages.push(BUSY.to_owned());

        let every = [
            NO_KEYRING,
            LOCKED,
            READ_DEADLINE,
            READ_FAILED,
            INVALID,
            DUPLICATES,
            BUSY,
            DISMISSED,
            SAVE_REFUSED,
            SAVED_MISSING,
            CHANGED,
            SENT_UNCONFIRMED,
            SENT_UNREAD,
            CONNECT_DEADLINE,
            CONNECT_DEADLINE_SENT,
            CONNECT_CANCELLED,
            CONNECT_CANCELLED_SENT,
            REMOVE_DISMISSED,
            REMOVE_FAILED,
            REMAINS,
            DISCONNECT_DEADLINE,
            DISCONNECT_DEADLINE_SENT,
            DISCONNECT_CANCELLED,
            DISCONNECT_CANCELLED_SENT,
        ];
        let seen = messages.iter().map(String::as_str).collect();
        assert_eq!(std::collections::BTreeSet::from(every), seen);
        for message in &messages {
            let lower = message.to_lowercase();
            assert!(!message.contains(MARKER) && !lower.contains("keyring detail"));
            assert!(!lower.contains("keychain") && !lower.contains("encrypt"));
        }
    }

    /// An account's whole life through the flows on one keyring, with a marker token:
    /// Connect, the stored read that follows, Disconnect, then a stored read the keyring
    /// fails with an error whose own text carries the token. Nothing returned carries the
    /// token in any form the panel or a log could show (`{}` as Disconnect's failure shows,
    /// `{:#}` with every cause as Connect's and Refresh's do, `{:?}`), and none names another
    /// platform's store or calls the keyring encrypted.
    #[test]
    fn an_accounts_life_in_the_keyring_shows_its_token_nowhere() {
        fn shown<T: std::fmt::Debug>(result: &Result<T>) -> Vec<String> {
            match result {
                Ok(value) => vec![format!("{value:?}")],
                Err(error) => vec![
                    error.to_string(),
                    format!("{error:#}"),
                    format!("{error:?}"),
                ],
            }
        }
        let control = OperationControl::default();
        // No default collection yet: Connect's save creates it.
        let fake = Fake::new(false, &[]);
        let connected = connect(&fake, &control);
        let mut texts = shown(&connected);
        connected.expect("Connect stores the account");
        // The token went through the flow, so it could have leaked.
        let created = fake.created.borrow();
        let [(_, _, secret)] = created.as_slice() else {
            panic!("Connect writes one item");
        };
        assert!(String::from_utf8_lossy(secret.as_bytes()).contains(MARKER));
        drop(created);
        let restored = read(&fake);
        texts.extend(shown(&restored));
        assert_eq!(restored.unwrap().expect("the stored account").token, MARKER);
        let removed = disconnect(&fake, &control);
        texts.extend(shown(&removed));
        removed.expect("Disconnect removes it");
        let emptied = read(&fake);
        texts.extend(shown(&emptied));
        assert!(emptied.unwrap().is_none());
        let failing = Fake {
            fail: Some(("secret", || {
                Error::NotFound(format!("keyring detail {MARKER}"))
            })),
            ..Fake::new(true, &[&stored_json()])
        };
        let failed = read(&failing);
        texts.extend(shown(&failed));
        assert_eq!(texts.len(), 7);
        for text in &texts {
            let lower = text.to_lowercase();
            assert!(
                !text.contains(MARKER) && !lower.contains("keyring detail"),
                "{text}"
            );
            for store in ["keychain", "encrypt", "secure storage"] {
                assert!(!lower.contains(store), "{store}: {text}");
            }
        }
        assert_eq!(message(failed), READ_FAILED);
    }

    #[test]
    fn a_test_build_reaches_a_real_keyring_only_on_a_private_test_bus() {
        use std::{ffi::OsStr, os::unix::net::UnixListener};
        // The constructor consults the guard (it never touches a bus); `connect` is left
        // untested, since a test calling it could reach a real bus without its guard.
        assert_eq!(DesktopKeyring::new().is_ok(), on_private_test_bus());
        // The socket's directory is made in /tmp, whose path is short on every host, and /tmp
        // stands for the temporary directory the guard measures the socket against: a socket's
        // path holds at most 107 bytes (`sun_path`), and under a 109-character TMPDIR, as a
        // private one under a worktree's `.local/` can be, `bind` refused `<TMPDIR>/.tmp…/bus`,
        // 124 bytes.
        let temp = std::path::Path::new("/tmp");
        let private = tempfile::tempdir_in(temp).unwrap();
        let socket = private.path().join("bus");
        let _listener = UnixListener::bind(&socket).unwrap();
        let file = private.path().join("file");
        std::fs::write(&file, b"").unwrap();
        let link = private.path().join("link");
        std::os::unix::fs::symlink(&socket, &link).unwrap();
        let address = |path: &std::path::Path| format!("unix:path={}", path.display());
        let bus = address(&socket);
        let accepts = |fixture: Option<&str>, session: Option<&str>, temp: &std::path::Path| {
            private_test_bus(fixture.map(OsStr::new), session.map(OsStr::new), temp).is_some()
        };
        assert!(accepts(Some(&bus), Some(&bus), temp));
        let with_guid = format!("{bus},guid=0123456789abcdef0123456789abcdef");
        assert!(accepts(Some(&with_guid), Some(&with_guid), temp));

        let elsewhere = tempfile::tempdir_in(temp).unwrap();
        let escaped = format!(
            "unix:path={}/../{}/bus",
            private.path().display(),
            private.path().file_name().unwrap().to_str().unwrap()
        );
        let refused = [
            (None, None),
            (None, Some(bus.clone())),
            (Some(bus.clone()), None),
            (Some(format!("{bus},guid=00")), Some(bus.clone())),
            (Some(address(&file)), Some(address(&file))),
            (Some(address(&link)), Some(address(&link))),
            (Some(escaped.clone()), Some(escaped)),
            (Some(format!("{bus};{bus}")), Some(format!("{bus};{bus}"))),
            (
                Some(format!("{bus},abstract=x")),
                Some(format!("{bus},abstract=x")),
            ),
            (
                Some("unix:abstract=bus".into()),
                Some("unix:abstract=bus".into()),
            ),
        ];
        for (fixture, session) in refused {
            assert!(!accepts(fixture.as_deref(), session.as_deref(), temp));
        }
        assert!(!accepts(Some(&bus), Some(&bus), elsewhere.path()));
    }

    /// Serializes the tests that call the public `load`, `save` and `remove`: their
    /// process-wide lock (`ACCESS`) refuses a second caller at once, so under plain
    /// `cargo test`, where tests share one process, one such test would refuse another.
    /// The flows beneath them need none.
    static SERIAL: Mutex<()> = Mutex::new(());
    fn serial() -> MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The real client against a disposable gnome-keyring on a private D-Bus, opt in, since
    /// they need `dbus-daemon` and `gnome-keyring-daemon` (Arch `dbus` and `gnome-keyring`).
    /// An outer test starts the daemons with nothing of the developer's session (no bus
    /// address, display or home), then runs an inner test in a fresh process of this test
    /// binary whose session bus is the private one, which the guard ([`FIXTURE_BUS`]) lets it
    /// reach; nothing here sets an environment variable in its own process or reaches the
    /// session bus the developer's shell runs on. Run directly, an inner test refuses at
    /// the guard before any bus call, so a broad `--ignored` run without `--exact` fails its
    /// inner tests by design: run the outer tests by name. The functional run:
    ///
    /// `cargo test --locked -p gitturtle --bin gitturtle -- --ignored --exact
    /// github::transport::secret_service::tests::hermetic::a_disposable_gnome_keyring_holds_the_account
    /// --nocapture`
    mod hermetic {
        use super::*;
        use std::{
            io::{BufRead as _, BufReader},
            os::unix::fs::DirBuilderExt as _,
            path::PathBuf,
            process::{Child, Command, Stdio},
            time::Instant,
        };

        /// This module's tests, as the test binary names them.
        const HERE: &str = "github::transport::secret_service::tests::hermetic::";
        /// How long the private bus and the keyring each have to come up.
        const START_LIMIT: Duration = Duration::from_secs(5);
        /// How long an inner test may run: its keyring calls end within their own bounds.
        const INNER_LIMIT: Duration = Duration::from_secs(60);
        /// Names the measurement's JSON output.
        const BENCH_OUT: &str = "GITTURTLE_KEYRING_BENCH_OUT";
        /// The stored reads the measurement times.
        const READS: usize = 50;
        const SECRETS: &str = "org.freedesktop.secrets";

        /// A private session bus and, once started, a disposable gnome-keyring on it, both
        /// kept in one temporary directory with their HOME, data, runtime and control
        /// directories. Dropping it kills and reaps both daemons, then removes the
        /// directory, whatever happened.
        struct PrivateKeyring {
            bus: Option<Child>,
            keyring: Option<Child>,
            address: String,
            directory: tempfile::TempDir,
        }
        impl Drop for PrivateKeyring {
            fn drop(&mut self) {
                for mut daemon in [self.keyring.take(), self.bus.take()].into_iter().flatten() {
                    let _ = daemon.kill();
                    let _ = daemon.wait();
                }
            }
        }
        impl PrivateKeyring {
            /// Starts the bus. Its configuration names no service directories, so nothing,
            /// the host's gnome-keyring included, is started on it by D-Bus activation, as
            /// `--session` would let the host's `org.freedesktop.secrets.service` do.
            fn start_bus() -> Self {
                let directory = tempfile::Builder::new()
                    .prefix("gitturtle-keyring-")
                    .tempdir()
                    .expect("a temporary directory");
                for part in ["home", "data", "config", "runtime", "keyring"] {
                    std::fs::DirBuilder::new()
                        .mode(0o700)
                        .create(directory.path().join(part))
                        .expect("a private directory");
                }
                let address = format!("unix:path={}", directory.path().join("bus").display());
                let configuration = directory.path().join("bus.conf");
                std::fs::write(
                    &configuration,
                    format!(
                        "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"\n \
                         \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                         <busconfig><type>session</type><listen>{address}</listen>\
                         <auth>EXTERNAL</auth><policy context=\"default\">\
                         <allow send_destination=\"*\" eavesdrop=\"true\"/>\
                         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n"
                    ),
                )
                .expect("the bus configuration");
                let mut keyring = Self {
                    bus: None,
                    keyring: None,
                    address: String::new(),
                    directory,
                };
                let mut bus = keyring
                    .daemon("dbus-daemon", "bus.log")
                    .arg(format!("--config-file={}", configuration.display()))
                    .args([
                        "--nofork",
                        &format!("--address={address}"),
                        "--print-address=1",
                    ])
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("dbus-daemon starts (install dbus)");
                let output = bus.stdout.take().expect("the bus's output");
                keyring.bus = Some(bus);
                // Its first line is the address with the bus's GUID. A read blocks, so it
                // runs beside, bounded.
                let (send, receive) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let mut line = String::new();
                    let _ = BufReader::new(output).read_line(&mut line);
                    let _ = send.send(line);
                });
                let printed = receive.recv_timeout(START_LIMIT).unwrap_or_default();
                assert!(
                    printed.starts_with(&address),
                    "the private bus did not start within 5 s: {}",
                    keyring.log("bus.log")
                );
                keyring.address = printed.trim().to_owned();
                keyring
            }

            /// Starts gnome-keyring on the bus with a synthetic password, which unlocks the
            /// login keyring it creates as the default, and waits, bounded, until it owns
            /// the Secret Service's name.
            fn start_keyring(&mut self) {
                let mut keyring = self
                    .daemon("gnome-keyring-daemon", "keyring.log")
                    .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
                    .args(["--foreground", "--components=secrets", "--unlock"])
                    .arg("--control-directory")
                    .arg(self.path("keyring"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .expect("gnome-keyring-daemon starts (install gnome-keyring)");
                let mut password = keyring.stdin.take().expect("the keyring's input");
                self.keyring = Some(keyring);
                password
                    .write_all(b"synthetic-password-for-a-disposable-keyring")
                    .expect("the keyring reads its password");
                drop(password);
                let started = Instant::now();
                while !self.owned(SECRETS) {
                    assert!(
                        started.elapsed() < START_LIMIT,
                        "the keyring did not take {SECRETS} within 5 s: {}",
                        self.log("keyring.log")
                    );
                    std::thread::sleep(Duration::from_millis(25));
                }
            }

            fn path(&self, part: &str) -> PathBuf {
                self.directory.path().join(part)
            }

            /// `program` with nothing of the developer's session, its error output in `log`.
            fn daemon(&self, program: &str, log: &str) -> Command {
                let log = std::fs::File::create(self.path(log)).expect("a daemon log");
                let mut command = Command::new(program);
                command
                    .env_clear()
                    .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                    .stdin(Stdio::null())
                    .stderr(log);
                self.private(&mut command);
                command
            }

            /// Gives `command` this directory's HOME and XDG directories, and the system
            /// temporary directory the guard measures the bus's socket against.
            fn private(&self, command: &mut Command) {
                command
                    .env("HOME", self.path("home"))
                    .env("XDG_DATA_HOME", self.path("data"))
                    .env("XDG_CONFIG_HOME", self.path("config"))
                    .env("XDG_RUNTIME_DIR", self.path("runtime"));
                if let Some(temporary) = std::env::var_os("TMPDIR") {
                    command.env("TMPDIR", temporary);
                }
            }

            fn log(&self, log: &str) -> String {
                std::fs::read_to_string(self.path(log)).unwrap_or_default()
            }

            /// Whether `name` has an owner on the private bus, asked through a connection to
            /// its address, never the session bus.
            fn owned(&self, name: &str) -> bool {
                let address = self.address.as_str();
                let owned = async {
                    let bus = oo7::zbus::connection::Builder::address(address)?
                        .build()
                        .await?;
                    let proxy = oo7::zbus::fdo::DBusProxy::new(&bus).await?;
                    Ok::<_, oo7::zbus::Error>(proxy.name_has_owner(name.try_into()?).await?)
                };
                futures::executor::block_on(owned).unwrap_or(false)
            }

            /// Runs `test`, one of this module's inner tests, in a fresh process of this test
            /// binary whose session bus is the private one, and checks that it ran and passed.
            fn run(&self, test: &str) {
                let mut command = Command::new(std::env::current_exe().expect("this test binary"));
                command
                    .args(["--ignored", "--exact", &format!("{HERE}{test}")])
                    .args(["--nocapture", "--test-threads=1"])
                    .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
                    .env(FIXTURE_BUS, &self.address)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped());
                self.private(&mut command);
                let mut inner = command.spawn().expect("this test binary runs again");
                let output = inner.stdout.take().expect("its output");
                let reader = std::thread::spawn(move || {
                    let mut text = String::new();
                    let _ = BufReader::new(output).read_to_string(&mut text);
                    text
                });
                let started = Instant::now();
                let status = loop {
                    if let Some(status) = inner.try_wait().expect("its status") {
                        break status;
                    }
                    if started.elapsed() > INNER_LIMIT {
                        let _ = inner.kill();
                        let _ = inner.wait();
                        panic!("{test} did not finish within {INNER_LIMIT:?}");
                    }
                    std::thread::sleep(Duration::from_millis(25));
                };
                let output = reader.join().unwrap_or_default();
                print!("{output}");
                // An exact name that matches no test runs none, and passes.
                assert!(
                    status.success() && output.contains("test result: ok. 1 passed"),
                    "{test} did not pass on the private bus: {status}"
                );
            }
        }

        /// A synthetic account: a token that is clearly no GitHub token.
        fn synthetic() -> Credential {
            let mut bytes = [0u8; 16];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut random| random.read_exact(&mut bytes))
                .expect("random bytes");
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            Credential {
                login: "fixture-login".into(),
                token: format!("not-a-token-{hex}"),
            }
        }

        /// The test's own calls to the private keyring's default collection, beside the
        /// flows', through the guarded connection.
        fn collection<T>(
            keyring: &DesktopKeyring,
            call: impl AsyncFnOnce(&oo7::dbus::Collection) -> Result<T, Error>,
        ) -> T {
            futures::executor::block_on(async {
                let session = keyring.connect().await.expect("the private bus");
                let collection = keyring
                    .stored_collection(&session)
                    .await
                    .expect("the default alias")
                    .expect("the disposable login keyring");
                call(&collection).await.expect("the keyring call")
            })
        }
        fn items(keyring: &DesktopKeyring) -> usize {
            let attributes: &[(&str, &str)] = &ATTRIBUTES;
            collection(keyring, async |collection| {
                Ok(collection.search_items(&attributes).await?.len())
            })
        }
        /// An item with GitTurtle's schema and `text`, beside any already there.
        fn write(keyring: &DesktopKeyring, text: &str) {
            let attributes: &[(&str, &str)] = &ATTRIBUTES;
            collection(keyring, async |collection| {
                let secret = Secret::text(text);
                collection
                    .create_item(LABEL, &attributes, secret, false, None)
                    .await
                    .map(drop)
            })
        }
        fn credential_json(credential: &Credential) -> String {
            serde_json::to_string(credential).expect("the account's JSON")
        }

        #[test]
        #[ignore = "starts dbus-daemon and gnome-keyring-daemon; run with --ignored --exact"]
        fn a_disposable_gnome_keyring_holds_the_account() {
            let mut keyring = PrivateKeyring::start_bus();
            // Before the keyring starts, nothing owns the Secret Service and nothing is
            // activated for it.
            keyring.run("on_a_private_bus_without_a_keyring");
            keyring.start_keyring();
            keyring.run("on_a_private_bus_with_a_keyring");
        }

        /// Inside the harness only (`a_disposable_gnome_keyring_holds_the_account`).
        #[test]
        #[ignore = "run by a_disposable_gnome_keyring_holds_the_account on its private bus"]
        fn on_a_private_bus_without_a_keyring() {
            let _serial = serial();
            let keyring = DesktopKeyring::new().expect("refused off a private test bus");
            let control = OperationControl::default();
            assert_eq!(message(load(&keyring)), NO_KEYRING);
            assert_eq!(message(save(&keyring, &synthetic(), &control)), NO_KEYRING);
            assert_eq!(message(remove(&keyring, &control)), NO_KEYRING);
        }

        /// Inside the harness only (`a_disposable_gnome_keyring_holds_the_account`).
        #[test]
        #[ignore = "run by a_disposable_gnome_keyring_holds_the_account on its private bus"]
        fn on_a_private_bus_with_a_keyring() {
            let _serial = serial();
            let keyring = DesktopKeyring::new().expect("refused off a private test bus");
            let control = OperationControl::default();
            let first = synthetic();
            save(&keyring, &first, &control).expect("Connect stores the account");
            let loaded = load(&keyring).unwrap().expect("the stored account");
            assert_eq!(
                (loaded.login, loaded.token),
                (first.login.clone(), first.token.clone())
            );
            // Connected again, the item is replaced, not added to.
            let second = synthetic();
            save(&keyring, &second, &control).expect("Connect replaces the account");
            assert_eq!(items(&keyring), 1);
            assert_eq!(
                load(&keyring).unwrap().expect("the account").token,
                second.token
            );
            remove(&keyring, &control).expect("Disconnect removes it");
            assert!(load(&keyring).unwrap().is_none());
            assert_eq!(items(&keyring), 0);

            // Two entries another client wrote: a stored read refuses them, and Disconnect
            // removes both.
            write(&keyring, &credential_json(&first));
            write(&keyring, &credential_json(&second));
            assert_eq!(items(&keyring), 2);
            assert_eq!(message(load(&keyring)), DUPLICATES);
            remove(&keyring, &control).expect("Disconnect removes both");
            assert_eq!(items(&keyring), 0);

            // An entry that is not the account's JSON.
            write(&keyring, "not the account's JSON");
            assert_eq!(message(load(&keyring)), INVALID);
            remove(&keyring, &control).expect("Disconnect removes it");

            // A locked keyring: the stored read refuses well within its bound and leaves the
            // keyring locked, so it awaited no unlock prompt.
            save(&keyring, &first, &control).expect("Connect stores the account");
            collection(&keyring, async |collection| collection.lock(None).await);
            let started = Instant::now();
            assert_eq!(message(load(&keyring)), LOCKED);
            assert!(started.elapsed() < READ_LIMIT, "{:?}", started.elapsed());
            assert!(collection(&keyring, async |collection| collection
                .is_locked()
                .await));
        }

        /// The release measurement of a stored read: 50 sequential reads on a disposable
        /// gnome-keyring, each a fresh connection, transfer session, search and secret
        /// (`Service::new()` through `load`), written as JSON (`samples_ms`, `median_ms`,
        /// `p95_ms`, `max_ms`, the host and oo7's version) to the file
        /// `GITTURTLE_KEYRING_BENCH_OUT` names. Build with
        /// `cargo test --locked --release -p gitturtle --bin gitturtle --no-run`, then run
        /// the executable it names (`target/release/deps/gitturtle-…`) on an idle host:
        ///
        /// `GITTURTLE_KEYRING_BENCH_OUT=keyring-reads.json <executable> --ignored --exact
        /// github::transport::secret_service::tests::hermetic::stored_reads_on_a_disposable_gnome_keyring
        /// --nocapture`
        #[test]
        #[ignore = "the release measurement; run with --ignored --exact"]
        fn stored_reads_on_a_disposable_gnome_keyring() {
            assert!(
                std::env::var_os(BENCH_OUT).is_some(),
                "{BENCH_OUT} names the measurement's JSON file"
            );
            let mut keyring = PrivateKeyring::start_bus();
            keyring.start_keyring();
            keyring.run("stored_reads_on_a_private_bus");
        }

        /// Inside the harness only (`stored_reads_on_a_disposable_gnome_keyring`).
        #[test]
        #[ignore = "run by stored_reads_on_a_disposable_gnome_keyring on its private bus"]
        fn stored_reads_on_a_private_bus() {
            let _serial = serial();
            let keyring = DesktopKeyring::new().expect("refused off a private test bus");
            let out = std::env::var_os(BENCH_OUT).expect("the measurement's JSON file");
            save(&keyring, &synthetic(), &OperationControl::default()).expect("the account");
            let samples: Vec<f64> = (0..READS)
                .map(|_| {
                    let started = Instant::now();
                    let read = load(&keyring);
                    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                    assert!(matches!(read, Ok(Some(_))), "a stored read failed");
                    elapsed
                })
                .collect();
            let mut sorted = samples.clone();
            sorted.sort_by(f64::total_cmp);
            let median = (sorted[READS / 2 - 1] + sorted[READS / 2]) / 2.0;
            let p95 = sorted[(READS * 95).div_ceil(100) - 1];
            let max = sorted[READS - 1];
            let read = |path: &str| {
                std::fs::read_to_string(path)
                    .map(|text| text.trim().to_owned())
                    .unwrap_or_default()
            };
            let measurement = serde_json::json!({
                "measurement": "stored GitHub account reads from a disposable gnome-keyring",
                "reads": READS,
                "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                "samples_ms": samples,
                "median_ms": median,
                "p95_ms": p95,
                "max_ms": max,
                "bound_ms": READ_LIMIT.as_millis() as u64,
                "host": {
                    "hostname": read("/proc/sys/kernel/hostname"),
                    "kernel": read("/proc/sys/kernel/osrelease"),
                    "arch": std::env::consts::ARCH,
                    "cpus": std::thread::available_parallelism().map_or(0, usize::from),
                },
                "oo7": oo7_version(),
            });
            std::fs::write(
                &out,
                serde_json::to_vec_pretty(&measurement).expect("the measurement's JSON"),
            )
            .expect("the measurement's file");
            println!("{measurement}");
        }

        /// oo7's version as the lockfile pins it.
        fn oo7_version() -> String {
            let lock = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"));
            lock.split("[[package]]")
                .find(|package| package.contains("\nname = \"oo7\"\n"))
                .and_then(|package| {
                    package
                        .lines()
                        .find_map(|line| line.strip_prefix("version = "))
                })
                .map(|version| version.trim_matches('"').to_owned())
                .unwrap_or_default()
        }
    }
}
