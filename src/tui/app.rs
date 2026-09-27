//! Screen state: what is in the list, what the selection points at, what is
//! asked of the engine — and the translation from the model to the text each
//! row shows.
//!
//! No network in here: the refresh runs on a thread and arrives through the
//! channel, one reading at a time (the list fills in before the user's eyes).
//! The engine state shown on screen is warnings and counts, never credential
//! values.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use chrono::{DateTime, Utc};

use crate::cache;
use crate::config::Config;
use crate::credentials::{self, Credentials, Source, expand_tilde};
use crate::engine::{self, Context, Filter, Refresh};
use crate::http::Http;
use crate::i18n;
use crate::keyfile;
use crate::model::{
    CACHE_VERSION, Cache, Category, Class, Cost, Level, Meter, Reading, Status, Unit,
};
use crate::providers::{Provider, UNDOCUMENTED};
use crate::secret::Secret;
use crate::t;
use crate::tui::theme::{self, CATALOG, ColorOverrides, Preferences, Theme};
use crate::tui::visibility::{self, Visibility};

/// Below this width no TUI is readable.
pub const MIN_WIDTH: u16 = 60;
/// Below this height no TUI is readable.
pub const MIN_HEIGHT: u16 = 12;

/// The sources the engine needs, shared with the refresh thread.
pub struct Sources {
    /// The provider registry, in order (it gives the screen its rows).
    pub providers: Arc<Vec<Box<dyn Provider>>>,
    /// The already validated configuration.
    pub config: Arc<Config>,
    /// The credentials already read — only the engine touches them.
    pub cred: Arc<Credentials>,
}

/// The screen in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The list of services.
    List,
    /// The detail of the selected service.
    Detail,
    /// The help, with this run's notes.
    Help,
    /// Language, theme and bar style (`p`), over the list.
    Preferences,
    /// The Services & keys screen (`s`): which services appear, and the keys.
    Services,
}

/// What the refresh thread sends through the channel.
#[derive(Debug)]
pub enum RefreshEvent {
    /// A reading that just arrived: shown right away, without waiting for the
    /// end.
    Reading(Box<Reading>),
    /// The refresh is over: the final readings, what was left out and when.
    Done {
        refresh: Box<Refresh>,
        now: DateTime<Utc>,
    },
}

/// A data row on the screen: a meter, or the status text of a reading that
/// has no meters at all.
#[derive(Debug, Clone)]
pub struct Row {
    /// Provider id — this is what the selection stores.
    pub provider: String,
    /// Name to show; only on the service's first row.
    pub service: String,
    pub category: Category,
    pub cost: Cost,
    /// First row of this service in the list (the others have no name).
    pub first: bool,
    /// Index of the meter within the reading; `None` on status rows.
    pub meter: Option<usize>,
    /// The meter label, in English as the provider emits it (the screen shows
    /// it through [`i18n::label`]).
    pub label: String,
    /// The meter's level; `None` on status rows (there is no number at all).
    pub level: Option<Level>,
    /// What the row shows in the values column.
    pub primary: String,
    /// What only fits when the column has room (the `reset …`).
    pub extra: String,
    /// **Remaining** fraction (0..=1), so a full bar means "plenty of room";
    /// `None` when there is no limit to compare against.
    pub bar: Option<f64>,
    /// Undocumented endpoint: the row gets a `†`.
    pub undocumented: bool,
    /// Muted row, because there is no credential to read the service.
    pub no_credential: bool,
    /// The reading failed (credential refused, network, API, format): the
    /// status row is painted as an error, not as missing data.
    pub failed: bool,
}

impl Row {
    /// Whether this row is the status text (and not a meter).
    pub fn is_status(&self) -> bool {
        self.level.is_none()
    }
}

/// The state of the Services & keys screen.
#[derive(Default)]
pub struct Menu {
    /// The row the cursor is on (index in the registry).
    pub cursor: usize,
    /// The key being typed, if the user pressed `a`.
    pub entry: Option<KeyEntry>,
    /// The result of the last action (text, and whether it is an error). It
    /// never carries values.
    pub message: Option<(String, bool)>,
}

/// A key being typed in the menu. The text is never drawn: the screen only
/// shows how many characters it has. Deliberately without `Debug`.
pub struct KeyEntry {
    /// The service the key belongs to.
    pub provider: String,
    /// The variables to ask for, in order (Twilio asks for two).
    pub variables: Vec<&'static str>,
    /// The variable being typed now.
    pub index: usize,
    /// What has been typed so far for the current variable.
    pub text: String,
    /// The previous variables, already confirmed.
    pub done: Vec<Secret>,
}

impl KeyEntry {
    /// The variable being typed.
    pub fn variable(&self) -> &'static str {
        self.variables[self.index]
    }
}

/// How many services are in each state, for the header summary. It counts
/// services, not meters: a service counts as its worst meter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub critical: usize,
    pub warnings: usize,
    pub ok: usize,
    pub failures: usize,
    pub no_data: usize,
}

/// What the TUI keeps from the engine and from the user.
pub struct App {
    pub sources: Sources,
    /// The readings to show, in the order they arrived: the list sorts them by
    /// the registry, not by this.
    pub readings: Vec<Reading>,
    /// Ids with no reading at all (neither now nor in the cache).
    pub unread: Vec<&'static str>,
    /// Warnings from the configuration, the credentials and the cache.
    pub warnings: Vec<String>,
    /// What the last refresh did: how many were left out, how many were within
    /// the minimum interval, how many have no reading.
    pub status_notes: Vec<String>,
    /// When the readings were taken (the age in the header).
    pub generated_at: Option<DateTime<Utc>>,
    /// The time of the frame: injectable, so tests do not depend on the clock.
    pub now: DateTime<Utc>,
    /// When the next automatic refresh is due; `None` with `interval_min = 0`.
    pub next_refresh: Option<DateTime<Utc>>,
    /// A refresh is running.
    pub refreshing: bool,
    /// How many readings arrived from the running refresh.
    pub received: usize,
    /// The screen in front.
    pub view: View,
    /// What the selection points at: provider id and meter index (`None` on
    /// status rows). Storing this, rather than an index, survives a reading
    /// that arrives midway and moves the list around.
    pub selected: Option<(String, Option<usize>)>,
    /// The `f` filter: hide rows without a credential and without a balance
    /// API.
    pub hide: bool,
    /// Where the cache is saved after each refresh; `None` when there is
    /// nowhere to save it (not an error).
    pub cache_path: Option<PathBuf>,
    /// The user asked to quit.
    pub quit: bool,
    /// The language, theme and bar in use.
    pub preferences: Preferences,
    /// The preferences before the Preferences screen was opened, so `Esc` can
    /// restore them.
    pub preferences_before: Option<Preferences>,
    /// The `[colors]` from `config.toml`, on top of any theme.
    pub overrides: ColorOverrides,
    /// Where the Preferences screen saves the choice; `None` when there is
    /// nowhere to save it.
    pub preferences_path: Option<PathBuf>,
    /// Which services appear on the home screen (the exceptions to the
    /// "services with a credential appear" rule).
    pub visibility: Visibility,
    /// Where the menu saves the visibility; `None` when there is nowhere to
    /// save it.
    pub services_path: Option<PathBuf>,
    /// The file where the menu writes keys (the first of `key_files`); `None`
    /// turns writing off.
    pub keys_file: Option<PathBuf>,
    /// Where the credentials are read again after the menu saves a key;
    /// `None` turns re-reading off.
    pub credential_source: Option<Source>,
    /// The Services & keys screen.
    pub menu: Menu,
    /// A read requested while another was running: launched when it ends.
    pub pending: Option<Filter>,
}

impl App {
    /// The initial state: the existing cache (if any) and nothing else. The
    /// refresh is launched afterwards, by [`crate::tui::run`]. The language
    /// starts as this thread's current one (set from `config.toml` and the
    /// saved preferences before the TUI starts).
    pub fn new(
        sources: Sources,
        previous_cache: Option<Cache>,
        now: DateTime<Utc>,
        warnings: Vec<String>,
        cache_path: Option<PathBuf>,
    ) -> App {
        let (readings, generated_at) = match previous_cache {
            Some(cache) => (cache.readings, Some(cache.generated_at)),
            None => (Vec::new(), None),
        };
        let selected = sources
            .providers
            .first()
            .map(|p| (p.id().to_string(), None));
        let mut warnings = warnings;
        let preferences = theme::resolve(
            sources.config.theme.as_deref(),
            sources.config.bar.as_deref(),
            None,
            &mut warnings,
        );
        let overrides = theme::overrides(&sources.config.colors, &mut warnings);
        App {
            next_refresh: next_refresh(now, sources.config.interval_min),
            sources,
            readings,
            unread: Vec::new(),
            warnings,
            status_notes: Vec::new(),
            generated_at,
            now,
            refreshing: false,
            received: 0,
            view: View::List,
            selected,
            hide: false,
            cache_path,
            quit: false,
            preferences,
            preferences_before: None,
            overrides,
            preferences_path: None,
            visibility: Visibility::default(),
            services_path: None,
            keys_file: None,
            credential_source: None,
            menu: Menu::default(),
            pending: None,
        }
    }

    /// Wires up what the real TUI needs and the tests do not: the saved
    /// visibility, the key file (the first of `key_files`) and the source to
    /// read the credentials again after saving one.
    pub fn attach_config(&mut self, services_path: Option<PathBuf>, source: Option<Source>) {
        if let Some(saved) = services_path.as_deref().and_then(visibility::read) {
            self.visibility = saved;
        }
        self.services_path = services_path;
        self.keys_file = self
            .sources
            .config
            .key_files
            .first()
            .map(|path| expand_tilde(path));
        self.credential_source = source;
        self.selected = self
            .rows()
            .first()
            .map(|row| (row.provider.clone(), row.meter));
    }

    /// Whether the service appears on the home screen.
    pub fn is_visible(&self, provider: &dyn Provider) -> bool {
        self.visibility
            .is_visible(provider.id(), self.has_credential(provider))
    }

    /// Whether there is a credential to read the service.
    pub fn has_credential(&self, provider: &dyn Provider) -> bool {
        visibility::has_credential(provider, &self.sources.cred)
    }

    /// The services that appear, in registry order.
    pub fn visible_providers(&self) -> Vec<&dyn Provider> {
        self.sources
            .providers
            .iter()
            .map(Box::as_ref)
            .filter(|provider| self.is_visible(*provider))
            .collect()
    }

    /// Opens the Services & keys screen, with the cursor on the selected
    /// service.
    pub fn open_services(&mut self) {
        let selected = self.selected.as_ref().map(|(id, _)| id.as_str());
        self.menu.cursor = self
            .sources
            .providers
            .iter()
            .position(|provider| Some(provider.id()) == selected)
            .unwrap_or(0);
        self.menu.entry = None;
        self.menu.message = None;
        self.view = View::Services;
    }

    /// Closes the menu (the choices were already saved one by one).
    pub fn close_services(&mut self) {
        self.menu.entry = None;
        self.view = View::List;
        if self.selected_index(&self.rows()).is_none() {
            self.move_by(isize::MIN);
        }
    }

    /// Moves the menu cursor (`g`/`G` arrive as `isize::MIN`/`MAX`).
    pub fn move_services_cursor(&mut self, step: isize) {
        let last = self.sources.providers.len().saturating_sub(1) as isize;
        self.menu.cursor = (self.menu.cursor as isize)
            .saturating_add(step)
            .clamp(0, last) as usize;
    }

    /// The provider under the menu cursor.
    fn menu_provider(&self) -> Option<&dyn Provider> {
        self.sources
            .providers
            .get(self.menu.cursor)
            .map(Box::as_ref)
    }

    /// Shows or hides the service under the cursor, and saves right away.
    /// Returns the read to do when the service becomes visible without any
    /// reading.
    pub fn toggle_visible(&mut self) -> Option<Filter> {
        let provider = self.menu_provider()?;
        let (id, name, has, cost) = (
            provider.id(),
            provider.service(),
            self.has_credential(provider),
            provider.cost(),
        );
        self.visibility.toggle(id, has);
        let now_visible = self.visibility.is_visible(id, has);
        self.save_visibility();
        let message = if now_visible {
            t!("tui.services.shown", service = name)
        } else {
            t!("tui.services.hidden", service = name)
        };
        self.menu.message = Some((message, false));
        (now_visible && self.reading_of(id).is_none() && cost == Cost::Free)
            .then(|| Filter::One(id.to_string()))
    }

    /// Starts typing the key of the service under the cursor.
    pub fn start_entry(&mut self) {
        let Some(provider) = self.menu_provider() else {
            return;
        };
        if !visibility::accepts_key(provider) {
            self.menu.message = Some((
                t!(
                    "tui.services.claude_not_key",
                    service = provider.service(),
                    variables = provider.variables().join(", ")
                ),
                true,
            ));
            return;
        }
        if self.keys_file.is_none() {
            self.menu.message = Some((t!("tui.services.no_key_file"), true));
            return;
        }
        self.menu.entry = Some(KeyEntry {
            provider: provider.id().to_string(),
            variables: provider.variables().to_vec(),
            index: 0,
            text: String::new(),
            done: Vec::new(),
        });
        self.menu.message = None;
    }

    /// A character typed into the entry.
    pub fn type_char(&mut self, character: char) {
        if let Some(entry) = &mut self.menu.entry
            && !character.is_control()
        {
            entry.text.push(character);
        }
    }

    /// Deletes the last character of the entry.
    pub fn backspace(&mut self) {
        if let Some(entry) = &mut self.menu.entry {
            entry.text.pop();
        }
    }

    /// Gives up on the key being typed.
    pub fn cancel_entry(&mut self) {
        self.menu.entry = None;
        self.menu.message = Some((t!("tui.services.nothing_saved"), false));
    }

    /// Confirms the current variable: moves on to the next one or, if it was
    /// the last, saves them all to the key file, reads the credentials again,
    /// makes the service visible and returns the read to do (only if free).
    pub fn confirm_entry(&mut self) -> Option<Filter> {
        let entry = self.menu.entry.as_mut()?;
        let text = entry.text.trim().to_string();
        if let Err(error) = keyfile::validate(&text) {
            self.menu.message = Some((format!("{}: {error}", entry.variable()), true));
            return None;
        }
        entry.text.clear();
        entry.done.push(Secret::new(text));
        if entry.index + 1 < entry.variables.len() {
            entry.index += 1;
            self.menu.message = None;
            return None;
        }

        let entry = self.menu.entry.take()?;
        let path = self.keys_file.clone()?;
        let pairs: Vec<(&str, &Secret)> = entry
            .variables
            .iter()
            .copied()
            .zip(entry.done.iter())
            .collect();
        if let Err(error) = keyfile::save(&path, &pairs) {
            self.menu.message = Some((
                t!(
                    "tui.services.key_save_failed",
                    path = path.display(),
                    error = error
                ),
                true,
            ));
            return None;
        }
        let mut message = t!(
            "tui.services.key_saved",
            variables = entry.variables.join(", "),
            path = path.display()
        );
        let mut error = false;

        // The environment wins over the files: if the variable is set there,
        // the new key only takes effect once it leaves the environment.
        if let Some(source) = &self.credential_source {
            let in_environment: Vec<&str> = entry
                .variables
                .iter()
                .copied()
                .filter(|name| source.env.iter().any(|(n, v)| n == name && !v.is_empty()))
                .collect();
            if !in_environment.is_empty() {
                message.push_str(&t!(
                    "tui.services.env_wins",
                    variables = in_environment.join(", ")
                ));
                error = true;
            }
            let mut warnings = Vec::new();
            self.sources.cred = Arc::new(credentials::load(source, &mut warnings));
            for warning in warnings {
                if !self.warnings.contains(&warning) {
                    self.warnings.push(warning);
                }
            }
        }

        let provider = self.provider_by_id(&entry.provider)?;
        let (id, has, cost) = (
            provider.id(),
            self.has_credential(provider),
            provider.cost(),
        );
        self.visibility.show(id, has);
        self.save_visibility();
        let request = if cost == Cost::Free {
            message.push_str(&t!("tui.services.reading"));
            Some(Filter::One(id.to_string()))
        } else {
            message.push_str(&t!("tui.services.paid"));
            None
        };
        self.menu.message = Some((message, error));
        request
    }

    /// Saves the visibility. A failure is a warning: the choice still holds
    /// for this run.
    fn save_visibility(&mut self) {
        if let Some(path) = &self.services_path
            && let Err(error) = visibility::save(path, &self.visibility)
        {
            self.warnings.push(t!(
                "tui.services.save_failed",
                path = path.display(),
                error = error
            ));
        }
    }

    /// The theme in use, with the `[colors]` overrides on top.
    pub fn theme(&self) -> Theme {
        self.preferences.theme().with_overrides(&self.overrides)
    }

    /// Reads the preferences saved by the Preferences screen (they win over
    /// `config.toml`) and saves the next choices there. A file without a
    /// `language` keeps the current language.
    pub fn load_preferences(&mut self, path: Option<PathBuf>) {
        if let Some(saved) = path.as_deref().and_then(theme::read) {
            self.preferences = saved;
        }
        self.preferences_path = path;
    }

    /// Opens the Preferences screen, remembering the current preferences so
    /// `Esc` can go back to them.
    pub fn open_preferences(&mut self) {
        self.preferences_before = Some(self.preferences);
        self.view = View::Preferences;
    }

    /// Moves to the previous/next theme in the catalog (applied right away, so
    /// it can be seen).
    pub fn move_theme(&mut self, step: isize) {
        let total = CATALOG.len() as isize;
        self.preferences.theme =
            (self.preferences.theme as isize + step).rem_euclid(total) as usize;
    }

    /// Moves to the next bar style.
    pub fn cycle_bar(&mut self) {
        self.preferences.bar = self.preferences.bar.next();
    }

    /// Moves to the next language (applied right away: the next frame is drawn
    /// in it).
    pub fn cycle_language(&mut self) {
        self.preferences.next_language();
    }

    /// Closes the Preferences screen, saving the choice. A failure to save is
    /// a warning: the choice stays in use for this run anyway.
    pub fn save_preferences(&mut self) {
        self.preferences_before = None;
        self.view = View::List;
        if let Some(path) = &self.preferences_path
            && let Err(error) = theme::save(path, &self.preferences)
        {
            // The warning is written in the language just chosen.
            i18n::set_current(self.preferences.language);
            self.warnings.push(t!(
                "tui.preferences.save_failed",
                path = path.display(),
                error = error
            ));
        }
    }

    /// Closes the Preferences screen without saving, restoring the previous
    /// preferences (language included).
    pub fn cancel_preferences(&mut self) {
        if let Some(before) = self.preferences_before.take() {
            self.preferences = before;
        }
        self.view = View::List;
    }

    /// The header summary: how many services are critical, in warning, fine,
    /// failed, and without data (no credential or no balance API).
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for provider in self.visible_providers() {
            let Some(reading) = self.reading_of(provider.id()) else {
                counts.no_data += 1;
                continue;
            };
            if !(matches!(reading.status, Status::Ok) && !reading.meters.is_empty()) {
                if failed(&reading.status) {
                    counts.failures += 1;
                } else {
                    counts.no_data += 1;
                }
                continue;
            }
            let levels = || reading.meters.iter().map(|m| m.level);
            if levels().any(|level| matches!(level, Level::Critical | Level::Exhausted)) {
                counts.critical += 1;
            } else if levels().any(|level| level == Level::Warning) {
                counts.warnings += 1;
            } else {
                counts.ok += 1;
            }
        }
        counts
    }

    /// The home screen rows: only the visible services, in a single list.
    /// Services with numbers come first, then the ones that failed and last
    /// the ones without data — each group in registry order.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for provider in self.visible_providers() {
            match self.reading_of(provider.id()) {
                Some(reading) => {
                    if !(self.hide && hidden_by_filter(reading)) {
                        self.reading_rows(provider, reading, &mut rows);
                    }
                }
                // No reading at all: a row that says so, so the service does
                // not vanish from the screen without explanation.
                None => {
                    if !self.hide {
                        rows.push(self.waiting_row(provider));
                    }
                }
            }
        }
        // `sort_by_key` is stable: registry order is kept within each group,
        // and a service's rows stay together (they all belong to the same
        // group).
        rows.sort_by_key(|row| (row.is_status(), !row.failed));
        rows
    }

    /// A provider's reading, if there is one yet.
    pub fn reading_of(&self, id: &str) -> Option<&Reading> {
        self.readings.iter().find(|reading| reading.provider == id)
    }

    /// The registry's provider, by id.
    pub fn provider_by_id(&self, id: &str) -> Option<&dyn Provider> {
        self.sources
            .providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(Box::as_ref)
    }

    /// The selected row, if any (cloned: the list is rebuilt on every reading
    /// that arrives).
    pub fn selected_row(&self) -> Option<Row> {
        let rows = self.rows();
        let index = self.selected_index(&rows).unwrap_or(0);
        rows.into_iter().nth(index)
    }

    /// The selected row among the given `rows` (the index the drawing uses);
    /// `None` if the selected service is no longer in the list.
    pub fn selected_index(&self, rows: &[Row]) -> Option<usize> {
        let (id, meter) = self.selected.as_ref()?;
        let exact = rows
            .iter()
            .position(|row| &row.provider == id && row.meter == *meter);
        // The service's reading may have changed shape (from a status row to
        // two meter rows, or the other way around): stay on its first row
        // instead of jumping to another service.
        exact.or_else(|| rows.iter().position(|row| &row.provider == id))
    }

    /// Moves the selection `step` rows.
    pub fn move_by(&mut self, step: isize) {
        let rows = self.rows();
        if rows.is_empty() {
            self.selected = None;
            return;
        }
        let current = self.selected_index(&rows).unwrap_or(0);
        // `g`/`G` arrive as `isize::MIN`/`MAX`: the sum saturates instead of
        // overflowing in debug builds.
        let target = (current as isize)
            .saturating_add(step)
            .clamp(0, rows.len() as isize - 1) as usize;
        let row = &rows[target];
        self.selected = Some((row.provider.clone(), row.meter));
    }

    /// The filter the selected row asks for (`r`: only that service, including
    /// the per-request ones the automatic refresh skips).
    pub fn selection_request(&self) -> Option<Filter> {
        self.selected
            .as_ref()
            .map(|(id, _)| Filter::One(id.clone()))
    }

    /// The `f` key: toggles hiding the rows without any number.
    pub fn toggle_filter(&mut self) {
        self.hide = !self.hide;
    }

    /// A reading arrived: it goes into the list right away (replacing the same
    /// service's, if any).
    pub fn receive(&mut self, reading: Reading) {
        self.received += 1;
        match self
            .readings
            .iter_mut()
            .find(|old| old.provider == reading.provider)
        {
            Some(old) => *old = reading,
            None => self.readings.push(reading),
        }
    }

    /// The refresh is over: keep the full result, the summary, and save the
    /// cache. The refresh only went through the visible services: the hidden
    /// ones keep their readings (in the cache too), so they come back if the
    /// services do.
    pub fn finish(&mut self, refresh: Refresh, now: DateTime<Utc>) {
        self.status_notes = summary(&refresh);
        self.unread = refresh.unread.clone();
        let mut readings = refresh.readings;
        for old in self.readings.drain(..) {
            if !readings.iter().any(|new| new.provider == old.provider) {
                readings.push(old);
            }
        }
        self.readings = readings;
        self.generated_at = Some(now);
        self.now = now;
        self.refreshing = false;
        self.received = 0;
        self.next_refresh = next_refresh(now, self.sources.config.interval_min);
        self.save_cache(now);
    }

    /// The automatic refresh, if it is due (it only reads the free services).
    pub fn refresh_due(&self, now: DateTime<Utc>) -> Option<Filter> {
        if self.refreshing {
            return None;
        }
        let next_refresh = self.next_refresh?;
        (now >= next_refresh).then_some(Filter::Automatic)
    }

    /// Launches a refresh on a thread, of the visible services only. Readings
    /// arrive one by one through `tx`; `Done` closes it.
    pub fn launch(&mut self, filter: Filter, tx: Sender<RefreshEvent>) {
        let visible_providers: Vec<&'static str> =
            self.visible_providers().iter().map(|p| p.id()).collect();
        let sources = Sources {
            providers: Arc::clone(&self.sources.providers),
            config: Arc::clone(&self.sources.config),
            cred: Arc::clone(&self.sources.cred),
        };
        let previous: BTreeMap<String, Reading> = self
            .readings
            .iter()
            .map(|reading| (reading.provider.clone(), reading.clone()))
            .collect();
        self.refreshing = true;
        self.received = 0;

        std::thread::spawn(move || {
            // The registry is rebuilt (it is cheap) so the engine only gets
            // the visible services: a hidden one is not read.
            let providers: Vec<Box<dyn Provider>> = engine::registry(&sources.config)
                .into_iter()
                .filter(|provider| visible_providers.contains(&provider.id()))
                .collect();
            let http = Http::new(sources.config.timeout_s);
            let context = Context {
                http: &http,
                cred: sources.cred.as_ref(),
                config: sources.config.as_ref(),
                now: Utc::now(),
            };
            // The engine speaks `Reading`; the screen speaks `RefreshEvent`. A
            // thread translates, so the engine does not need to know about the
            // screen.
            let (reading_tx, reading_rx) = channel::<Reading>();
            let bridge = tx.clone();
            let translator = std::thread::spawn(move || {
                for reading in reading_rx {
                    if bridge
                        .send(RefreshEvent::Reading(Box::new(reading)))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            let refresh = engine::refresh(&providers, &filter, &previous, &context, reading_tx);
            let _ = translator.join();
            let _ = tx.send(RefreshEvent::Done {
                refresh: Box::new(refresh),
                now: Utc::now(),
            });
        });
    }

    /// The notes for the header/help: what this run has to explain.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.hide {
            notes.push(t!("tui.notes.filter"));
        }
        notes.extend(self.status_notes.iter().cloned());
        notes.extend(self.warnings.iter().cloned());
        notes
    }

    /// The state in the header: what is happening, or when it happens next.
    pub fn refresh_state(&self) -> String {
        if self.refreshing {
            return t!("tui.header.reading", n = self.received);
        }
        match self.next_refresh {
            Some(next_refresh) => t!(
                "tui.header.next",
                duration = relative_duration(next_refresh, self.now)
            ),
            None => t!("tui.header.auto_off"),
        }
    }

    /// How long ago the readings were taken ("3m ago"), or that there are
    /// none yet.
    pub fn age_short(&self) -> String {
        match self.generated_at {
            Some(when) => relative_duration(when, self.now),
            None => t!("tui.header.no_readings"),
        }
    }

    /// The age of the readings, for the header ("updated 3m ago").
    pub fn age(&self) -> String {
        match self.generated_at {
            // `relative_duration` already says "… ago" when the time is in
            // the past.
            Some(when) => t!(
                "tui.header.updated",
                when = relative_duration(when, self.now)
            ),
            None => t!("tui.header.no_readings"),
        }
    }

    /// Saves the cache after a refresh. The cache is disposable: a failure is
    /// a warning, never a startup error.
    fn save_cache(&mut self, now: DateTime<Utc>) {
        let Some(path) = self.cache_path.clone() else {
            return;
        };
        let new = Cache {
            version: CACHE_VERSION,
            generated_at: now,
            readings: self.readings.clone(),
        };
        // The anti-leak guard is the same as `--json`'s: the text goes through
        // `cache::prepare` before touching the disk. The borrow of the secrets
        // ends here, before the warnings are touched.
        let prepared = {
            let secrets = self.sources.cred.all_exposed();
            cache::prepare(&new, &secrets)
        };
        match prepared {
            Ok(text) => {
                if let Err(error) = cache::save(&path, &text) {
                    self.warnings.push(error.to_string());
                }
            }
            Err(error) => self.warnings.push(error.to_string()),
        }
    }

    /// The rows of a reading: one per meter, or a single one with the status
    /// text when there are no meters.
    fn reading_rows(&self, provider: &dyn Provider, reading: &Reading, rows: &mut Vec<Row>) {
        let undocumented = UNDOCUMENTED.contains(&provider.id());
        let no_credential = matches!(reading.status, Status::NoCredential);
        if matches!(reading.status, Status::Ok) && !reading.meters.is_empty() {
            for (index, meter) in reading.meters.iter().enumerate() {
                let (primary, extra) = meter_values(meter, reading.class, self.now);
                rows.push(Row {
                    provider: provider.id().to_string(),
                    service: provider.service().to_string(),
                    category: provider.category(),
                    cost: provider.cost(),
                    first: index == 0,
                    meter: Some(index),
                    label: meter.label.clone(),
                    level: Some(meter.level),
                    primary,
                    extra,
                    bar: remaining_fraction(meter),
                    undocumented,
                    no_credential,
                    failed: false,
                });
            }
            return;
        }
        rows.push(Row {
            provider: provider.id().to_string(),
            service: provider.service().to_string(),
            category: provider.category(),
            cost: provider.cost(),
            first: true,
            meter: None,
            label: String::new(),
            level: None,
            primary: status_row_text(reading, provider),
            extra: String::new(),
            bar: None,
            undocumented,
            no_credential,
            failed: failed(&reading.status),
        });
    }

    /// The row of a service that has no reading at all.
    fn waiting_row(&self, provider: &dyn Provider) -> Row {
        Row {
            provider: provider.id().to_string(),
            service: provider.service().to_string(),
            category: provider.category(),
            cost: provider.cost(),
            first: true,
            meter: None,
            label: String::new(),
            level: None,
            primary: if self.unread.contains(&provider.id()) {
                t!("tui.status.never_read")
            } else {
                t!("tui.status.waiting")
            },
            extra: String::new(),
            bar: None,
            undocumented: UNDOCUMENTED.contains(&provider.id()),
            no_credential: false,
            failed: false,
        }
    }
}

/// What the `f` filter hides: the rows that have no number at all to show.
pub fn hidden_by_filter(reading: &Reading) -> bool {
    matches!(reading.status, Status::NoCredential) || reading.class == Class::NoApi
}

/// Whether the reading failed — as opposed to merely having no data to give
/// (no credential, no supported endpoint).
pub fn failed(status: &Status) -> bool {
    !matches!(
        status,
        Status::Ok | Status::NoCredential | Status::Unsupported
    )
}

/// The next automatic refresh, if `interval_min` turns it on.
fn next_refresh(now: DateTime<Utc>, interval_min: u32) -> Option<DateTime<Utc>> {
    if interval_min == 0 {
        return None;
    }
    Some(now + chrono::Duration::minutes(i64::from(interval_min)))
}

/// The summary of a refresh, one note per line. The notes are built in the
/// language current when the refresh ends.
fn summary(refresh: &Refresh) -> Vec<String> {
    let mut notes = Vec::new();
    if !refresh.within_interval.is_empty() {
        notes.push(t!(
            "tui.notes.within_interval",
            n = refresh.within_interval.len(),
            seconds = engine::MIN_INTERVAL_S,
            ids = refresh.within_interval.join(", ")
        ));
    }
    if !refresh.filtered_out.is_empty() {
        notes.push(t!(
            "tui.notes.filtered_out",
            n = refresh.filtered_out.len(),
            ids = refresh.filtered_out.join(", ")
        ));
    }
    if !refresh.unread.is_empty() {
        notes.push(t!(
            "tui.notes.unread",
            n = refresh.unread.len(),
            ids = refresh.unread.join(", ")
        ));
    }
    notes
}

/// The status as a row shows it, in the current language. It never carries
/// credential values.
pub fn status_text(status: &Status) -> String {
    match status {
        Status::Ok => t!("tui.status.ok"),
        Status::NoCredential => t!("tui.status.no_credential"),
        Status::InvalidCredential { http } => t!("tui.status.invalid_credential", http = http),
        // Only Claude has a credential that expires, and the app does not
        // renew it: the hint is all it gives, with no network request.
        Status::ExpiredCredential => t!("tui.status.expired_credential"),
        Status::RateLimited { http } => t!("tui.status.rate_limited", http = http),
        Status::ApiError { http, message } => {
            t!("tui.status.api_error", http = http, message = message)
        }
        Status::NetworkError { message } => t!("tui.status.network_error", message = message),
        Status::UnexpectedFormat { message } => {
            t!("tui.status.unexpected_format", message = message)
        }
        Status::Unsupported => t!("tui.status.unsupported"),
    }
}

/// The text of a reading without meters: a missing credential names the
/// missing variable, and a service without a balance API says just that.
pub fn status_row_text(reading: &Reading, provider: &dyn Provider) -> String {
    if matches!(reading.status, Status::NoCredential) {
        return t!(
            "tui.status.no_credential_for",
            variables = provider.variables().join(", ")
        );
    }
    if reading.class == Class::NoApi {
        let base = t!("tui.status.no_api");
        return match reading.status {
            Status::Ok => base,
            _ => format!("{base} · {}", status_text(&reading.status)),
        };
    }
    status_text(&reading.status)
}

/// A meter's value as the row shows it: no extra decimals, in the current
/// language, with the unit after it; and the reset time, when there is one.
fn meter_values(meter: &Meter, class: Class, now: DateTime<Utc>) -> (String, String) {
    let extra = match meter.resets_at {
        Some(when) => t!("tui.value.reset", when = relative_duration(when, now)),
        None => String::new(),
    };
    (meter_primary(meter, class), extra)
}

/// The essentials of a meter — what the row shows even when it is narrow.
fn meter_primary(meter: &Meter, class: Class) -> String {
    if meter.unit == Unit::Percent {
        return match meter.used {
            Some(used) => t!("tui.value.percent_used", value = number(used, 1)),
            None => match meter.remaining {
                Some(remaining) => t!("tui.value.percent_left", value = number(remaining, 1)),
                None => no_numbers(),
            },
        };
    }
    if meter.unit == Unit::Currency {
        let currency = meter.currency.as_deref().unwrap_or("");
        return match (meter.remaining, meter.limit, meter.used) {
            (Some(remaining), Some(limit), _) => {
                format!("{} / {} {currency}", number(remaining, 2), number(limit, 2))
            }
            (Some(remaining), None, _) => format!("{} {currency}", number(remaining, 2)),
            (None, Some(limit), Some(used)) => {
                format!("{} / {} {currency}", number(used, 2), number(limit, 2))
            }
            (None, Some(limit), None) => format!("{} {currency}", number(limit, 2)),
            (None, None, _) => no_numbers(),
        };
    }
    let unit = unit_name(meter.unit);
    match (meter.remaining, meter.limit, meter.used) {
        // Rate limit only: what is left of the window (Brave, Context7, GitHub).
        (Some(remaining), Some(limit), _) if class == Class::RateLimitOnly => t!(
            "tui.value.left_of",
            remaining = number(remaining, 0),
            limit = number(limit, 0),
            unit = unit
        ),
        // Exact balance with a limit but no known usage (Firecrawl): the
        // balance is what matters, and the plan explains the limit.
        (Some(remaining), Some(limit), None) if class == Class::ExactBalance => t!(
            "tui.value.balance_of_plan",
            remaining = number(remaining, 0),
            unit = unit,
            limit = number(limit, 0)
        ),
        // Usage vs limit: what has been spent must be visible (Tavily,
        // ElevenLabs).
        (_, Some(limit), Some(used)) => t!(
            "tui.value.used_of",
            used = number(used, 0),
            limit = number(limit, 0),
            unit = unit
        ),
        (Some(remaining), Some(limit), None) => t!(
            "tui.value.left_of",
            remaining = number(remaining, 0),
            limit = number(limit, 0),
            unit = unit
        ),
        (Some(remaining), None, _) => format!("{} {unit}", number(remaining, 0)),
        (None, Some(limit), None) => {
            t!("tui.value.limit_of", limit = number(limit, 0), unit = unit)
        }
        (None, None, Some(used)) => t!("tui.value.used", used = number(used, 0), unit = unit),
        (None, None, None) => no_numbers(),
    }
}

/// A meter without any number does not make one up: it shows there is none.
fn no_numbers() -> String {
    "—".to_string()
}

/// The unit's name in the current language, for the screen row.
pub fn unit_name(unit: Unit) -> String {
    match unit {
        Unit::Currency | Unit::Credits => t!("tui.unit.credits"),
        Unit::Tokens => t!("tui.unit.tokens"),
        Unit::Characters => t!("tui.unit.characters"),
        Unit::Requests => t!("tui.unit.requests"),
        Unit::Percent => "%".to_string(),
    }
}

/// A meter's **remaining** fraction: it is what the bar draws, so a full side
/// always means "plenty of room".
pub fn remaining_fraction(meter: &Meter) -> Option<f64> {
    let limit = meter.limit?;
    if limit <= 0.0 {
        return None;
    }
    let remaining = meter
        .remaining
        .or_else(|| meter.used.map(|used| limit - used))?;
    Some((remaining / limit).clamp(0.0, 1.0))
}

/// A number with the current language's decimal separator, with no extra
/// decimals and no trailing zeros.
///
/// `0` is the tricky case: without the `contains('.')` guard every zero would
/// be trimmed and a real zero would come out blank.
pub fn number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    text.replace('.', &i18n::decimal_separator().to_string())
}

/// An **exact** number, as it is in the model (the detail panel shows it this
/// way, without rounding).
pub fn exact(value: f64) -> String {
    value
        .to_string()
        .replace('.', &i18n::decimal_separator().to_string())
}

/// A duration in days/hours/minutes, two units at most ("4d 12h", "1h 46m",
/// "59m" in English).
pub fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (days, hours, minutes) = (
        seconds / 86_400,
        (seconds % 86_400) / 3_600,
        (seconds % 3_600) / 60,
    );
    if days > 0 {
        return if hours > 0 {
            t!("tui.duration.days_hours", days = days, hours = hours)
        } else {
            t!("tui.duration.days", days = days)
        };
    }
    if hours > 0 {
        return if minutes > 0 {
            t!(
                "tui.duration.hours_minutes",
                hours = hours,
                minutes = minutes
            )
        } else {
            t!("tui.duration.hours", hours = hours)
        };
    }
    if minutes > 0 {
        return t!("tui.duration.minutes", minutes = minutes);
    }
    t!("tui.duration.seconds", seconds = seconds)
}

/// How long until `when` (or since, "… ago") from `now`, in the rows' format.
pub fn relative_duration(when: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = when.signed_duration_since(now).num_seconds();
    if seconds < 0 {
        return t!("tui.duration.ago", duration = duration(-seconds));
    }
    duration(seconds)
}
