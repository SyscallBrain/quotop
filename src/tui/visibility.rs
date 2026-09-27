//! Which services appear on the home screen.
//!
//! The default rule is "services with a credential appear": with nothing
//! configured, the screen shows only the configured APIs, and a new key in
//! `keys.env` makes its service appear on its own. The Services & keys screen
//! (`s`) only stores the **exceptions** to that rule — services to show even
//! without a key and services to hide even with one — in
//! `~/.local/state/quotop/services.toml`.
//!
//! A hidden service is not read either: no request is spent on what is not
//! shown (the same idea as `disabled` in `config.toml`, which removes a
//! service from the registry altogether).

use std::collections::BTreeSet;
use std::path::Path;

use crate::credentials::Credentials;
use crate::providers::Provider;

/// Name of the file where the Services & keys screen saves the choices.
pub const FILE_NAME: &str = "services.toml";

/// Id of the only service whose credential is not a variable (it is Claude
/// Code's credentials file).
const CLAUDE: &str = "claude";

/// The exceptions to the "services with a credential appear" rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Ids to show even without a credential.
    pub show: BTreeSet<String>,
    /// Ids to hide even with a credential.
    pub hide: BTreeSet<String>,
}

impl Visibility {
    /// Whether the service appears on the home screen.
    pub fn is_visible(&self, id: &str, has_credential: bool) -> bool {
        if self.hide.contains(id) {
            return false;
        }
        has_credential || self.show.contains(id)
    }

    /// Toggles a service's visibility. When the toggle brings it back to the
    /// default rule, the exception goes away (the file keeps only what was
    /// actually chosen).
    pub fn toggle(&mut self, id: &str, has_credential: bool) {
        let becomes_visible = !self.is_visible(id, has_credential);
        self.show.remove(id);
        self.hide.remove(id);
        if becomes_visible != has_credential {
            let list = if becomes_visible {
                &mut self.show
            } else {
                &mut self.hide
            };
            list.insert(id.to_string());
        }
    }

    /// Forces a service to appear (it just got a key from the menu).
    pub fn show(&mut self, id: &str, has_credential: bool) {
        if !self.is_visible(id, has_credential) {
            self.toggle(id, has_credential);
        }
    }

    /// The text [`save`] writes.
    pub fn to_toml(&self) -> String {
        let list = |ids: &BTreeSet<String>| {
            ids.iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "# Written by the quotop Services & keys screen (key `s`). Safe to delete:\n\
             # without it, the services that have a credential are shown.\n\
             show = [{}]\nhide = [{}]\n",
            list(&self.show),
            list(&self.hide)
        )
    }
}

/// Whether there is a credential to read the service: all of its variables
/// (Twilio needs two), or Claude Code's credentials file.
pub fn has_credential(provider: &dyn Provider, cred: &Credentials) -> bool {
    if provider.id() == CLAUDE {
        return cred.claude().is_some();
    }
    provider.variables().iter().all(|name| cred.has(name))
}

/// Whether the menu can write this service's key (all but Claude, whose
/// credential belongs to Claude Code).
pub fn accepts_key(provider: &dyn Provider) -> bool {
    provider.id() != CLAUDE && !provider.variables().is_empty()
}

/// Reads the saved choices. A missing or unreadable file is the same as no
/// choices at all.
pub fn read(path: &Path) -> Option<Visibility> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let list = |key: &str| -> BTreeSet<String> {
        table
            .get(key)
            .and_then(|value| value.as_array())
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Visibility {
        show: list("show"),
        hide: list("hide"),
    })
}

/// Saves the choices (creating the directory if needed), through a temporary
/// file and a rename.
pub fn save(path: &Path, visibility: &Visibility) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("toml.tmp");
    std::fs::write(&temporary, visibility.to_toml())?;
    std::fs::rename(&temporary, path)
}
