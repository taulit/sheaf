//! One app per session: the first launch owns `io.github.taulit.Sheaf` and serves
//! `org.freedesktop.Application`, every later launch hands its request to it and exits.
//!
//! Nothing here touches GPUI. [`start`] runs before any GUI work, because D-Bus
//! activation calls `Activate` as soon as the name appears; requests wait in a channel
//! until the app takes them.

use std::collections::HashMap;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use sheaf_searchd::api::{
    ACTION_NEW_NOTE, ACTION_OPEN_NOTE, ACTION_QUICK_NOTE, ACTION_SEARCH, APP_NAME, APP_PATH,
    ApplicationProxy, action_parameter,
};
use zbus::connection::Builder;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::{Connection, OwnedVariant, Variant};

/// Keys of `platform_data`, see the Desktop Entry specification, "D-Bus Activation".
const ACTIVATION_TOKEN: &str = "activation-token";
const STARTUP_ID: &str = "desktop-startup-id";

/// A running instance may still be starting; D-Bus activation alone can take seconds.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(10);

/// What a launch, a D-Bus caller or the desktop asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Show the main window.
    Activate,
    /// Create a note and open it.
    NewNote,
    /// Open the Quick Note window.
    QuickNote,
    /// Open a note; jump to the first match of `query` if it is not empty.
    OpenNote { id: String, query: String },
    /// Show the gallery with this search.
    Search { query: String },
    /// Open files, given as URIs.
    OpenUris(Vec<String>),
}

impl Request {
    /// The action name and string parameters this request travels as in
    /// `ActivateAction`. `None` for the requests that have a method of their own.
    pub fn action(&self) -> Option<(&'static str, Vec<&str>)> {
        match self {
            Request::Activate | Request::OpenUris(_) => None,
            Request::NewNote => Some((ACTION_NEW_NOTE, Vec::new())),
            Request::QuickNote => Some((ACTION_QUICK_NOTE, Vec::new())),
            Request::OpenNote { id, query } => Some((ACTION_OPEN_NOTE, vec![id, query])),
            Request::Search { query } => Some((ACTION_SEARCH, vec![query])),
        }
    }

    /// The request behind an `ActivateAction` call. `None` for an unknown action or one
    /// that lacks its parameters.
    pub fn from_action(name: &str, params: &[String]) -> Option<Request> {
        let param = |index: usize| params.get(index).cloned();
        match name {
            ACTION_NEW_NOTE => Some(Request::NewNote),
            ACTION_QUICK_NOTE => Some(Request::QuickNote),
            ACTION_OPEN_NOTE => Some(Request::OpenNote {
                id: param(0).filter(|id| !id.is_empty())?,
                query: param(1).unwrap_or_default(),
            }),
            ACTION_SEARCH => Some(Request::Search {
                query: param(0).unwrap_or_default(),
            }),
            _ => None,
        }
    }
}

/// A [`Request`] with the token that lets the app take focus for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activation {
    pub request: Request,
    /// The xdg-activation token (Wayland) or startup id (X11) of whoever asked.
    pub token: Option<String>,
}

impl Activation {
    /// `request` with the token this process was started with.
    pub fn from_launch(request: Request) -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Activation {
            request,
            token: var("XDG_ACTIVATION_TOKEN").or_else(|| var("DESKTOP_STARTUP_ID")),
        }
    }
}

/// How this process relates to the app on the bus.
#[derive(Debug)]
pub enum Instance {
    /// This process is the app. `connection` is `None` without a session bus; then
    /// nothing ever arrives on `requests`.
    Primary {
        connection: Option<Connection>,
        requests: Receiver<Activation>,
    },
    /// Another process is the app and has taken the request.
    Forwarded,
}

/// Which bus to look for the app on.
#[derive(Clone, Copy, Debug)]
pub enum Bus<'a> {
    Session,
    /// A bus by address, for tests on a private `dbus-daemon`.
    Address(&'a str),
}

/// Claims the app's bus name, or forwards `launch` to the process that owns it.
///
/// Blocks until the bus has answered. An error means a running instance exists but did
/// not take the request.
pub fn start(bus: Bus<'_>, launch: &Activation) -> zbus::Result<Instance> {
    let (sender, requests) = async_channel::unbounded();
    zbus::block_on(async {
        let Some(connection) = connect(bus, sender).await else {
            return Ok(Instance::Primary {
                connection: None,
                requests,
            });
        };
        let claimed = connection
            .request_name_with_flags(APP_NAME, RequestNameFlags::DO_NOT_QUEUE)
            .await;
        match claimed {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                Ok(Instance::Primary {
                    connection: Some(connection),
                    requests,
                })
            }
            Ok(_) | Err(zbus::Error::NameTaken) => {
                forward(&connection, launch).await?;
                Ok(Instance::Forwarded)
            }
            Err(err) => Err(err),
        }
    })
}

/// A connection that already serves the app's interface, so that no call is lost
/// between claiming the name and being ready. `None` if there is no bus.
async fn connect(bus: Bus<'_>, sender: Sender<Activation>) -> Option<Connection> {
    let builder = match bus {
        Bus::Session => Builder::session(),
        Bus::Address(address) => Builder::address(address),
    };
    let built = builder
        .method_timeout(FORWARD_TIMEOUT)
        .serve_at(APP_PATH, Application { sender })
        .build();
    match built.await {
        Ok(connection) => Some(connection),
        Err(err) => {
            log::info!("no session bus, running as a single process: {err}");
            None
        }
    }
}

async fn forward(connection: &Connection, launch: &Activation) -> zbus::Result<()> {
    let app = ApplicationProxy::new(connection).await?;
    let data = platform_data(launch.token.as_deref());
    match &launch.request {
        Request::Activate => app.activate(data).await,
        Request::OpenUris(uris) => {
            let uris: Vec<&str> = uris.iter().map(String::as_str).collect();
            app.open(&uris, data).await
        }
        request => {
            let Some((name, params)) = request.action() else {
                return Ok(());
            };
            app.activate_action(name, &action_parameter(&params), data)
                .await
        }
    }
}

/// The token under both keys: Wayland compositors read the first, X11 window managers
/// the second.
fn platform_data(token: Option<&str>) -> HashMap<&'static str, Variant<'_>> {
    token
        .into_iter()
        .flat_map(|token| [ACTIVATION_TOKEN, STARTUP_ID].map(|key| (key, Variant::from(token))))
        .collect()
}

fn token_of(platform_data: &HashMap<String, OwnedVariant>) -> Option<String> {
    [ACTIVATION_TOKEN, STARTUP_ID]
        .into_iter()
        .filter_map(|key| platform_data.get(key))
        .filter_map(|value| String::try_from(value.clone()).ok())
        .find(|token| !token.is_empty())
}

/// The strings of an action: the single `as` item of `parameter`, see
/// [`action_parameter`]. Anything else counts as no parameters.
fn action_strings(parameter: Vec<OwnedVariant>) -> Vec<String> {
    parameter
        .into_iter()
        .next()
        .and_then(|item| <Vec<String>>::try_from(item).ok())
        .unwrap_or_default()
}

struct Application {
    sender: Sender<Activation>,
}

impl Application {
    fn deliver(&self, request: Request, platform_data: &HashMap<String, OwnedVariant>) {
        let activation = Activation {
            request,
            token: token_of(platform_data),
        };
        // The receiver lives as long as the app; a failure means it is shutting down.
        let _ = self.sender.try_send(activation);
    }
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl Application {
    fn activate(&self, platform_data: HashMap<String, OwnedVariant>) {
        self.deliver(Request::Activate, &platform_data);
    }

    fn open(&self, uris: Vec<String>, platform_data: HashMap<String, OwnedVariant>) {
        self.deliver(Request::OpenUris(uris), &platform_data);
    }

    fn activate_action(
        &self,
        action_name: String,
        parameter: Vec<OwnedVariant>,
        platform_data: HashMap<String, OwnedVariant>,
    ) {
        match Request::from_action(&action_name, &action_strings(parameter)) {
            Some(request) => self.deliver(request, &platform_data),
            None => log::warn!("ignoring the unknown or incomplete action {action_name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(variants: Vec<Variant<'static>>) -> Vec<OwnedVariant> {
        variants
            .into_iter()
            .map(|variant| OwnedVariant::try_from(variant).expect("no file descriptors"))
            .collect()
    }

    #[test]
    fn requests_survive_the_trip_through_an_action() {
        let requests = [
            Request::NewNote,
            Request::QuickNote,
            Request::OpenNote {
                id: "01JD3K8Z6Q4X7V9B2M5N8P0R3T".to_owned(),
                query: "milch kaufen".to_owned(),
            },
            Request::OpenNote {
                id: "01JD3K8Z6Q4X7V9B2M5N8P0R3T".to_owned(),
                query: String::new(),
            },
            Request::Search {
                query: "#rezepte offen:".to_owned(),
            },
        ];
        for request in requests {
            let (name, params) = request.action().expect("travels as an action");
            let wire = owned(action_parameter(&params));
            assert!(wire.len() <= 1, "the specification allows one item");
            let decoded = Request::from_action(name, &action_strings(wire));
            assert_eq!(decoded.as_ref(), Some(&request));
        }
        assert_eq!(Request::Activate.action(), None);
        assert_eq!(Request::OpenUris(Vec::new()).action(), None);
    }

    #[test]
    fn broken_actions_are_refused() {
        assert_eq!(Request::from_action("delete-everything", &[]), None);
        assert_eq!(Request::from_action(ACTION_OPEN_NOTE, &[]), None);
        assert_eq!(
            Request::from_action(ACTION_OPEN_NOTE, &[String::new()]),
            None
        );
        assert_eq!(
            Request::from_action(ACTION_OPEN_NOTE, &["p123".to_owned()]),
            Some(Request::OpenNote {
                id: "p123".to_owned(),
                query: String::new()
            })
        );
        assert_eq!(
            Request::from_action(ACTION_SEARCH, &[]),
            Some(Request::Search {
                query: String::new()
            })
        );
    }

    #[test]
    fn parameters_that_are_not_one_string_array_count_as_none() {
        assert!(action_strings(Vec::new()).is_empty());
        assert!(action_strings(owned(vec![Variant::from(7_u32)])).is_empty());
        assert_eq!(
            action_strings(owned(vec![Variant::from(vec!["a".to_owned()])])),
            ["a"]
        );
    }

    #[test]
    fn the_token_travels_under_both_keys() {
        assert!(platform_data(None).is_empty());
        let data = platform_data(Some("token-1"));
        assert_eq!(data.len(), 2);
        let received: HashMap<String, OwnedVariant> = data
            .into_iter()
            .map(|(key, value)| {
                let value = OwnedVariant::try_from(value).expect("no file descriptors");
                (key.to_owned(), value)
            })
            .collect();
        assert_eq!(token_of(&received).as_deref(), Some("token-1"));

        let startup_only: HashMap<String, OwnedVariant> = received
            .into_iter()
            .filter(|(key, _)| key == STARTUP_ID)
            .collect();
        assert_eq!(token_of(&startup_only).as_deref(), Some("token-1"));
        assert_eq!(token_of(&HashMap::new()), None);
    }
}
