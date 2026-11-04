//! The GitHub account in the desktop keyring on Linux, through the Secret Service
//! (`docs/development/2026-10-28-linux-github-secret-service.md`). One item in the
//! collection behind the `default` alias holds the same JSON `{login, token}` the macOS
//! item holds. A stored read never asks the keyring to unlock or create anything and
//! ends within five seconds. Every failure fails closed: there is no file or other
//! fallback, and no message carries the token or a keyring error's own text.
//!
//! Each call opens one fresh session (`Backend::connect`), which survives a restarted
//! keyring daemon, and drives oo7's futures with `block_on` on the caller's thread: the
//! window's operation executor, never the UI thread. The flows run against a fake
//! `Backend` in tests, so they need no bus.
// LP3-K2 wires these into `SecureStore`; until then only this module's tests call them.
#![allow(dead_code)]
use super::*;
use futures::future::{Either, select};
use oo7::{Secret, dbus::Error};
use std::{future::Future, pin::pin};

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

const NO_KEYRING: &str = "No desktop keyring answered. Start your desktop's keyring (gnome-keyring on Omarchy), then try again.";
const LOCKED: &str = "Your desktop keyring is locked. Unlock it, or choose Connect GitHub CLI account, then retry. Stored account reads never open an unlock prompt.";
const READ_DEADLINE: &str = "Your desktop keyring did not answer within 5 seconds. Try again.";
const READ_FAILED: &str = "Your desktop keyring could not read the GitHub authorization. Try again, or choose Connect GitHub CLI account.";
const INVALID: &str = "The GitHub keyring entry is invalid. Connect the account again.";
const DUPLICATES: &str = "Your desktop keyring holds more than one GitTurtle GitHub entry. Choose Disconnect GitTurtle to remove them, then connect again.";

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
    pub(super) fn new() -> Result<Self> {
        Ok(Self(()))
    }
}
impl Backend for DesktopKeyring {
    type Session = oo7::dbus::Service;
    type Collection = oo7::dbus::Collection;
    type Item = oo7::dbus::Item;
    async fn connect(&self) -> Result<Self::Session, Error> {
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

/// The stored authorization, for Refresh and opening a pull request: never a prompt,
/// and an answer within [`READ_LIMIT`].
pub(super) fn load(backend: &impl Backend) -> Result<Option<Credential>> {
    futures::executor::block_on(read_within(backend, deadline(READ_LIMIT)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::oneshot;
    use std::cell::{Cell, RefCell};

    /// The call that fails, and the error it returns.
    type Failing = (&'static str, fn() -> Error);
    /// A keyring in memory. Each call is logged by name; `fail` makes one call return an
    /// error, and `hang` makes one call never finish after it fires `fire`.
    struct Fake {
        alias: Cell<bool>,
        locked: Cell<bool>,
        item_locked: bool,
        items: RefCell<Vec<Option<Secret>>>,
        log: RefCell<Vec<&'static str>>,
        searched: RefCell<Vec<Vec<(String, String)>>>,
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
            _: &str,
            _: &[(&str, &str)],
            secret: Secret,
        ) -> Result<(), Error> {
            self.enter("create_item").await?;
            self.items.borrow_mut().push(Some(secret));
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
            self.items.borrow_mut()[*item] = None;
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
}
