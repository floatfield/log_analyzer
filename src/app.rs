//! Application state and UI: file opening, background indexing/scanning,
//! the virtualized log table, column configuration, row selection with a
//! JSON detail pane, and the query input.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_extras::Column;
use memmap2::Mmap;
use serde_json::Value;

use crate::log_file::{self, IncrementalIndexer, IndexData};
use crate::persistence::{self, FilterModifier, Workspace};
use crate::query::{self, Query};

/// How many bytes the incremental indexer scans per pass.
const INDEX_CHUNK_BYTES: usize = 4 << 20;
/// Lines scanned between progress publishes / lock acquisitions.
const SCAN_BATCH: usize = 4096;
/// Query debounce interval (design D9).
const QUERY_DEBOUNCE: Duration = Duration::from_millis(250);
/// Rows the parse cache holds (design D3).
const PARSE_CACHE_CAPACITY: usize = 4096;
/// Rendered cell text is truncated beyond this many characters.
const MAX_CELL_CHARS: usize = 5000;
/// Default visible columns when a file opens.
const DEFAULT_COLUMNS: [&str; 3] = ["@timestamp", "level", "message"];
/// Translucent row tint for `error`-level rows (spec: log-table-view); light
/// alpha keeps text readable over the light theme.
const ERROR_ROW_TINT: egui::Color32 = egui::Color32::from_rgba_unmultiplied_const(220, 50, 50, 60);
/// Translucent row tint for `warn`/`warning`-level rows.
const WARN_ROW_TINT: egui::Color32 = egui::Color32::from_rgba_unmultiplied_const(235, 185, 30, 60);
/// Translucent tint marking the selected row, painted over any severity tint.
const SELECTED_ROW_TINT: egui::Color32 =
    egui::Color32::from_rgba_unmultiplied_const(70, 130, 220, 55);
/// The detail pane truncates very long contents beyond this many characters.
const DETAIL_MAX_CHARS: usize = 20_000;

type SharedIndex = Arc<RwLock<IndexData>>;
type SharedFields = Arc<Mutex<BTreeSet<String>>>;

/// A successfully opened, memory-mapped log file plus its shared state.
pub struct LoadedFile {
    pub path: PathBuf,
    pub map: Arc<Mmap>,
    pub index: SharedIndex,
    pub fields: SharedFields,
    /// Cancels the background indexing + field-discovery jobs.
    pub open_cancel: Arc<AtomicBool>,
    /// User-selected visible columns, in display order.
    pub visible_columns: Vec<String>,
    /// Set once default columns were picked from discovered fields, so later
    /// user edits are never overwritten.
    pub defaults_applied: bool,
}

impl LoadedFile {
    /// Memory-map `path`. Indexing and field discovery are started separately
    /// by [`LogAnalyzerApp::spawn_open_jobs`]; the shared index starts empty.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        // Safety-free variant: read-only mapping of a regular file.
        let map = unsafe { Mmap::map(&file)? };
        Ok(Self {
            path: path.to_path_buf(),
            map: Arc::new(map),
            index: Arc::new(RwLock::new(IndexData::default())),
            fields: Arc::new(Mutex::new(BTreeSet::new())),
            open_cancel: Arc::new(AtomicBool::new(false)),
            visible_columns: Vec::new(),
            defaults_applied: false,
        })
    }

    /// True once background indexing has finished.
    pub fn index_done(&self) -> bool {
        self.index.read().map(|i| i.done).unwrap_or(false)
    }

    /// Current line count known to the index.
    pub fn line_count(&self) -> usize {
        self.index.read().map(|i| i.line_count()).unwrap_or(0)
    }

    /// Pick default columns from discovered fields.
    pub fn apply_default_columns(&mut self) {
        let fields = self.fields.lock().unwrap();
        for name in DEFAULT_COLUMNS {
            if fields.contains(name) {
                self.visible_columns.push(name.to_owned());
            }
        }
        if self.visible_columns.is_empty() {
            self.visible_columns = fields.iter().take(3).cloned().collect();
        }
    }
}

/// Handle to a running (or finished) background query scan.
#[derive(Clone)]
pub struct ScanHandle {
    pub line_numbers: Arc<RwLock<Vec<u32>>>,
    pub scanned: Arc<AtomicUsize>,
    pub done: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
}

impl ScanHandle {
    fn new() -> Self {
        Self {
            line_numbers: Arc::new(RwLock::new(Vec::new())),
            scanned: Arc::new(AtomicUsize::new(0)),
            done: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Which rows the table currently shows.
#[derive(Clone, Default)]
pub enum RowSource {
    #[default]
    All,
    Matched(ScanHandle),
}

impl RowSource {
    /// Total rows currently visible for this source given the index.
    pub fn row_count(&self, index: &IndexData) -> usize {
        match self {
            RowSource::All => index.line_count(),
            RowSource::Matched(handle) => handle.line_numbers.read().map(|v| v.len()).unwrap_or(0),
        }
    }

    /// Map a table row index to a file line number.
    pub fn line_number(&self, row: usize, index: &IndexData) -> Option<usize> {
        match self {
            RowSource::All => {
                if row < index.line_count() {
                    Some(row)
                } else {
                    None
                }
            }
            RowSource::Matched(handle) => handle
                .line_numbers
                .read()
                .ok()
                .and_then(|v| v.get(row).copied())
                .map(|n| n as usize),
        }
    }
}

/// Core of the background query scan: stream every indexed line, evaluate the
/// query, collect matching line numbers, and piggyback field discovery
/// (design D6). Stops early when `cancel` is set.
pub fn run_query_scan(
    map: &[u8],
    index: &IndexData,
    query: &Query,
    out: &RwLock<Vec<u32>>,
    fields: &Mutex<BTreeSet<String>>,
    cancel: &AtomicBool,
    scanned: &AtomicUsize,
) {
    let needs_text = query.needs_line_text();
    let mut matches_buffer: Vec<u32> = Vec::new();
    let mut fields_buffer: Vec<String> = Vec::new();
    for line in 0..index.line_count() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Some(slice) = index.line_slice(map, line) {
            let entry = log_file::parse_object(slice);
            let matched = {
                let text_owned;
                let text: &str = if needs_text {
                    text_owned = String::from_utf8_lossy(slice).into_owned();
                    &text_owned
                } else {
                    ""
                };
                query.matches(text, entry.as_ref())
            };
            if matched {
                matches_buffer.push(line as u32);
            }
            if let Some(map_obj) = entry {
                fields_buffer.extend(map_obj.keys().cloned());
            }
        }
        if (line + 1) % SCAN_BATCH == 0 || line + 1 == index.line_count() {
            if let Ok(mut out_guard) = out.write() {
                out_guard.append(&mut matches_buffer);
            }
            if !fields_buffer.is_empty()
                && let Ok(mut fields_guard) = fields.lock()
            {
                fields_guard.extend(fields_buffer.drain(..));
            }
            scanned.store(line + 1, Ordering::Relaxed);
        }
    }
    if let Ok(mut out_guard) = out.write() {
        out_guard.append(&mut matches_buffer);
    }
    scanned.store(index.line_count(), Ordering::Relaxed);
}

/// Small LRU cache of parsed rows so back-and-forth scrolling avoids
/// reparsing (design D3).
struct ParseCache {
    capacity: usize,
    entries: HashMap<usize, Option<Arc<serde_json::Map<String, Value>>>>,
    order: VecDeque<usize>,
}

impl ParseCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get_or_compute(
        &mut self,
        key: usize,
        compute: impl FnOnce() -> Option<serde_json::Map<String, Value>>,
    ) -> Option<Arc<serde_json::Map<String, Value>>> {
        if let Some(hit) = self.entries.get(&key) {
            let hit = hit.clone();
            self.touch(key);
            return hit;
        }
        let value = compute().map(Arc::new);
        if self.entries.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.order.push_back(key);
        self.entries.insert(key, value.clone());
        value
    }

    fn touch(&mut self, key: usize) {
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            self.order.remove(pos);
            self.order.push_back(key);
        }
    }
}

/// UI state of the query input.
struct QueryUi {
    input: String,
    /// Input text the current row source was built from.
    applied_input: String,
    invalid: Option<String>,
    last_edit: Option<Instant>,
    /// Set by [`QueryUi::replace_filter_term`]; the next `poll` takes it and
    /// ORs it into the force argument so the replacement term applies without
    /// the debounce wait (design D5). One-shot.
    force_next_apply: bool,
}

impl QueryUi {
    fn new() -> Self {
        Self {
            input: String::new(),
            applied_input: String::new(),
            invalid: None,
            last_edit: None,
            force_next_apply: false,
        }
    }

    fn note_edit(&mut self) {
        self.last_edit = Some(Instant::now());
    }

    /// One-action reset (spec: log-query / Query reset control): clears the
    /// input and any invalid-query indication. Callers restore the all-rows
    /// view via [`LogAnalyzerApp::reset_rows_to_all`].
    fn reset(&mut self) {
        self.input.clear();
        self.applied_input.clear();
        self.invalid = None;
        self.last_edit = None;
        self.force_next_apply = false;
    }

    /// Install a generated filter term (spec: modifier-click-filtering /
    /// Filter term from a modifier-click): the term *replaces* the input's
    /// contents — any previous query text is discarded — then forces an apply
    /// so the rows filter without the debounce wait (design D8).
    fn replace_filter_term(&mut self, term: &str) {
        self.input = term.to_owned();
        self.note_edit();
        self.force_next_apply = true;
    }

    /// Decide what to apply this frame: nothing, the parsed query, or an
    /// invalid-query notice. Enter (`force`) bypasses the debounce.
    fn take_pending_apply(&mut self, force: bool) -> Option<Result<Query, String>> {
        let changed = self.input != self.applied_input;
        if !changed {
            return None;
        }
        let debounced = self
            .last_edit
            .is_some_and(|t| t.elapsed() >= QUERY_DEBOUNCE);
        if !force && !debounced {
            return None;
        }
        self.applied_input = self.input.clone();
        self.last_edit = None;
        let trimmed = self.input.trim();
        if trimmed.is_empty() {
            self.invalid = None;
            return Some(Ok(Query::default()));
        }
        match query::parse(trimmed) {
            Ok(q) => {
                self.invalid = None;
                Some(Ok(q))
            }
            Err(e) => {
                self.invalid = Some(e.message.clone());
                Some(Err(e.message))
            }
        }
    }

    /// Time until a pending edit's debounce elapses, if an apply is still
    /// owed (used to keep the UI repainting without further input events).
    fn pending_debounce_wait(&self) -> Option<Duration> {
        if self.input == self.applied_input {
            return None;
        }
        Some(
            self.last_edit
                .map(|t| QUERY_DEBOUNCE.saturating_sub(t.elapsed()))
                .unwrap_or(QUERY_DEBOUNCE),
        )
    }
}

/// Everything the table renderer needs, cloned out of the app state up front
/// to keep borrows simple inside egui closures.
struct TableView {
    map: Arc<Mmap>,
    index: SharedIndex,
    rows: RowSource,
    columns: Vec<String>,
}

impl FilterModifier {
    /// True when `modifiers` holds this setting's key. `Command` maps to
    /// egui's platform command modifier: Cmd on macOS, Ctrl elsewhere
    /// (design D1).
    fn matches_modifier(self, modifiers: egui::Modifiers) -> bool {
        match self {
            FilterModifier::Ctrl => modifiers.ctrl,
            FilterModifier::Alt => modifiers.alt,
            FilterModifier::Shift => modifiers.shift,
            FilterModifier::Command => modifiers.command,
        }
    }
}

/// All per-file session state for one opened log, presented as one tab
/// (spec: log-tabs). Switching tabs swaps this whole struct into view —
/// no reindex, rescan, or state copying (design D1/D2).
struct OpenTab {
    file: LoadedFile,
    rows: RowSource,
    query: QueryUi,
    cache: ParseCache,
    /// The selected row's file line number, if any; drives the detail pane.
    selected_line: Option<usize>,
    /// Columns-panel field-name filter text (session-only, spec:
    /// log-table-view).
    column_filter: String,
}

impl OpenTab {
    fn new(file: LoadedFile) -> Self {
        Self {
            file,
            rows: RowSource::All,
            query: QueryUi::new(),
            cache: ParseCache::new(PARSE_CACHE_CAPACITY),
            selected_line: None,
            column_filter: String::new(),
        }
    }

    /// Cancel any running query scan and reset the row source to all rows.
    fn cancel_scan(&mut self) {
        if let RowSource::Matched(handle) = &self.rows {
            handle.cancel.store(true, Ordering::Relaxed);
        }
        self.rows = RowSource::All;
    }

    /// Stop all of the tab's background work: open jobs (indexing, field
    /// discovery) and any running query scan.
    fn cancel_background_work(&mut self) {
        self.file.open_cancel.store(true, Ordering::Relaxed);
        self.cancel_scan();
    }
}

pub struct LogAnalyzerApp {
    /// One tab per opened file (spec: log-tabs).
    tabs: Vec<OpenTab>,
    /// Index into `tabs` of the tab the UI shows.
    active: usize,
    error: Option<String>,
    /// Keyboard modifier that turns a cell click into a query filter (spec:
    /// modifier-click-filtering). Mirrors `workspace.filter_modifier`.
    filter_modifier: FilterModifier,
    /// Persisted workspace: favorites and per-file column selections (spec:
    /// workspace-persistence).
    workspace: Workspace,
    /// Where the workspace persists; `None` degrades to in-memory only.
    workspace_path: Option<PathBuf>,
}

impl LogAnalyzerApp {
    pub fn new() -> Self {
        Self::with_workspace(persistence::default_config_path())
    }

    /// Constructor with an explicit workspace file location; `None` gives an
    /// in-memory workspace (design D4). Tests inject temp paths.
    fn with_workspace(config_path: Option<PathBuf>) -> Self {
        let workspace = config_path
            .as_deref()
            .map(Workspace::load)
            .unwrap_or_default();
        Self {
            tabs: Vec::new(),
            active: 0,
            error: None,
            filter_modifier: workspace.filter_modifier,
            workspace,
            workspace_path: config_path,
        }
    }

    /// The tab the UI currently shows, if any file is open.
    fn active_tab(&self) -> Option<&OpenTab> {
        self.tabs.get(self.active)
    }

    /// Mutable access to the tab the UI currently shows.
    fn active_tab_mut(&mut self) -> Option<&mut OpenTab> {
        self.tabs.get_mut(self.active)
    }

    /// Change the filter modifier and persist immediately (spec:
    /// modifier-click-filtering / Configurable filter modifier).
    fn set_filter_modifier(&mut self, modifier: FilterModifier) {
        self.filter_modifier = modifier;
        self.workspace.filter_modifier = modifier;
        self.save_workspace();
    }

    /// Write the workspace through to disk; failures are non-fatal.
    fn save_workspace(&self) {
        if let Some(path) = &self.workspace_path
            && let Err(e) = self.workspace.save(path)
        {
            eprintln!("failed to save workspace: {e}");
        }
    }

    /// Path key of the active tab's file, if any.
    fn current_file_key(&self) -> Option<String> {
        self.active_tab()
            .map(|tab| persistence::path_key(&tab.file.path))
    }

    /// Toggle the favorite flag for the open file and persist; returns the
    /// new state (spec: workspace-persistence / Managing favorites).
    fn toggle_favorite_current(&mut self) -> Option<bool> {
        let key = self.current_file_key()?;
        let now_favorite = self.workspace.toggle_favorite(&key);
        self.save_workspace();
        Some(now_favorite)
    }

    /// Remove a favorite by path key and persist.
    fn remove_favorite(&mut self, key: &str) {
        self.workspace.remove_favorite(key);
        self.save_workspace();
    }

    /// Show/hide a column for the open file and write the selection through
    /// to the workspace (spec: workspace-persistence).
    fn set_column_visible(&mut self, name: &str, on: bool) {
        let update = {
            let Some(tab) = self.active_tab_mut() else {
                return;
            };
            if on {
                if !tab.file.visible_columns.iter().any(|c| c == name) {
                    tab.file.visible_columns.push(name.to_owned());
                }
            } else {
                tab.file.visible_columns.retain(|c| c != name);
            }
            (
                persistence::path_key(&tab.file.path),
                tab.file.visible_columns.clone(),
            )
        };
        self.workspace.set_columns(&update.0, update.1);
        self.save_workspace();
    }

    /// Close the tab at `index`: stop its background work and remove it.
    /// Focus follows the removal — an earlier tab's removal shifts the strip
    /// (so the same tab stays in view), closing the active tab moves focus to
    /// its left neighbor, and closing the last tab leaves the empty state
    /// (spec: log-tabs / Closing a tab).
    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let mut tab = self.tabs.remove(index);
        tab.cancel_background_work();
        if index == self.active {
            self.active = self.active.saturating_sub(1);
        } else if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }

    /// Start background jobs for a freshly opened file: incremental index
    /// build followed by field discovery (design D2/D6).
    fn spawn_open_jobs(&mut self) {
        let Some(tab) = self.active_tab() else { return };
        let map = Arc::clone(&tab.file.map);
        let index = Arc::clone(&tab.file.index);
        let fields = Arc::clone(&tab.file.fields);
        let cancel = Arc::clone(&tab.file.open_cancel);
        std::thread::spawn(move || {
            let mut indexer = IncrementalIndexer::new();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let finished = {
                    let mut guard = match index.write() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    indexer.scan_chunk(&map, &mut guard, INDEX_CHUNK_BYTES)
                };
                if finished {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            // Field discovery after indexing; batched so the UI sees fields
            // appear progressively.
            let line_count = match index.read() {
                Ok(guard) => guard.line_count(),
                Err(_) => return,
            };
            for batch_start in (0..line_count).step_by(SCAN_BATCH) {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let index_guard = match index.read() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                let batch_end = (batch_start + SCAN_BATCH).min(line_count);
                let mut fields_guard = match fields.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                for line in batch_start..batch_end {
                    if let Some(slice) = index_guard.line_slice(&map, line)
                        && let Some(obj) = log_file::parse_object(slice)
                    {
                        fields_guard.extend(obj.keys().cloned());
                    }
                }
                drop(fields_guard);
                drop(index_guard);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
    }

    /// Open a path from the UI (spec: log-tabs). A tab already showing this
    /// file is focused as-is — no re-read and no state reset — otherwise the
    /// file is read and pushed as a new tab that becomes active; other tabs'
    /// background work continues untouched. Failures report in the status bar
    /// and leave every tab as it was.
    pub(crate) fn open_path(&mut self, path: &Path) {
        let key = persistence::path_key(path);
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| persistence::path_key(&tab.file.path) == key)
        {
            self.active = index;
            self.error = None;
            return;
        }
        match LoadedFile::open(path) {
            Ok(file) => {
                self.tabs.push(OpenTab::new(file));
                self.active = self.tabs.len() - 1;
                self.error = None;
                self.spawn_open_jobs();
            }
            Err(e) => {
                self.error = Some(format!("failed to open file: {e}"));
            }
        }
    }

    /// Re-open the current file from disk into its own tab (spec:
    /// log-file-access / Reloading the current file). The active query text
    /// is kept and re-applied to the reloaded contents once indexing
    /// completes.
    fn reload_current(&mut self) {
        let Some(path) = self.active_tab().map(|tab| tab.file.path.clone()) else {
            return;
        };
        let result = LoadedFile::open(&path);
        let opened = result.is_ok();
        self.on_open_result_inner(result);
        if opened {
            self.spawn_open_jobs();
            if let Some(tab) = self.active_tab_mut()
                && !tab.query.input.trim().is_empty()
            {
                // Re-arm the query so `poll` re-applies it after indexing
                // finishes (its retry loop keeps the query pending while the
                // index is still building).
                tab.query.applied_input = String::new();
                tab.query.last_edit = Some(Instant::now());
            }
        }
    }

    /// Install the outcome of re-reading the active tab's file (a reload):
    /// the tab is replaced at its strip position with the fresh read, keeping
    /// the query text (the caller re-arms it) and the columns-panel filter;
    /// the row selection is dropped — line numbers may shift after a reload.
    /// On failure the previously loaded state is retained (spec:
    /// log-file-access). Column selection is not picked here: field discovery
    /// has not run yet, so [`LogAnalyzerApp::maybe_apply_columns`] applies it
    /// once fields appear.
    fn on_open_result_inner(&mut self, result: io::Result<LoadedFile>) {
        match result {
            Ok(file) => {
                let index = self.active.min(self.tabs.len().saturating_sub(1));
                if index >= self.tabs.len() {
                    // No tab is open; `reload_current` guards against this,
                    // but installing as the first tab keeps the method total.
                    self.tabs.push(OpenTab::new(file));
                    self.active = 0;
                } else {
                    let mut old = std::mem::replace(&mut self.tabs[index], OpenTab::new(file));
                    old.cancel_background_work();
                    let fresh = &mut self.tabs[index];
                    fresh.query = old.query;
                    fresh.column_filter = old.column_filter;
                }
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("failed to open file: {e}"));
            }
        }
    }

    /// Start a background scan for `query`, replacing any running one.
    fn start_query_scan(&mut self, query: Query) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let map = Arc::clone(&tab.file.map);
        let index = Arc::clone(&tab.file.index);
        let fields = Arc::clone(&tab.file.fields);
        tab.cancel_scan();
        let handle = ScanHandle::new();
        let ScanHandle {
            line_numbers,
            scanned,
            done,
            cancel,
        } = handle.clone();
        tab.rows = RowSource::Matched(handle);
        std::thread::spawn(move || {
            let index_guard = match index.read() {
                Ok(g) => g,
                Err(_) => {
                    done.store(true, Ordering::Relaxed);
                    return;
                }
            };
            run_query_scan(
                &map,
                &index_guard,
                &query,
                &line_numbers,
                &fields,
                &cancel,
                &scanned,
            );
            done.store(true, Ordering::Relaxed);
        });
    }

    /// Show every row (empty/invalid query or cleared input).
    fn reset_rows_to_all(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.cancel_scan();
        }
    }

    /// Apply the per-file column selection once field discovery has produced
    /// names: the selection saved in the workspace when present, otherwise
    /// defaults (spec: workspace-persistence / Restoring saved columns). Runs
    /// at most once per file so user column edits afterwards are preserved.
    fn maybe_apply_columns(&mut self) {
        let should = self.active_tab().is_some_and(|tab| {
            !tab.file.defaults_applied && !tab.file.fields.lock().unwrap().is_empty()
        });
        if !should {
            return;
        }
        let saved = self
            .active_tab()
            .map(|tab| {
                let key = persistence::path_key(&tab.file.path);
                self.workspace.columns_for(&key).cloned()
            })
            .unwrap_or_default();
        if let Some(tab) = self.active_tab_mut() {
            tab.file.defaults_applied = true;
            match saved {
                Some(columns) => tab.file.visible_columns = columns,
                None => tab.file.apply_default_columns(),
            }
        }
    }

    /// Per-frame work: apply pending queries (debounce or Enter), keep
    /// repainting while background work is in flight.
    fn poll(&mut self, ctx: &egui::Context) {
        self.maybe_apply_columns();
        let mut force = self.enter_pressed_this_frame(ctx);
        let pending = match self.active_tab_mut() {
            Some(tab) => {
                // A modifier-click just replaced the query input: apply it
                // without waiting out the debounce (design D5).
                if std::mem::take(&mut tab.query.force_next_apply) {
                    force = true;
                }
                tab.query.take_pending_apply(force)
            }
            None => None,
        };
        match pending {
            Some(Ok(q)) => {
                if q.is_empty() {
                    self.reset_rows_to_all();
                } else if self
                    .active_tab()
                    .is_some_and(|tab| tab.file.index_done())
                {
                    self.start_query_scan(q);
                }
                // While indexing is still running the query stays pending:
                // applied_input was already updated, so re-arm the debounce.
                else if let Some(tab) = self.active_tab_mut() {
                    tab.query.last_edit = Some(Instant::now());
                    tab.query.applied_input = String::new();
                }
            }
            Some(Err(_)) => {
                // Invalid query: show all rows unfiltered (spec: log-query).
                self.reset_rows_to_all();
            }
            None => {
                // A pending edit must repaint once its debounce elapses even
                // with no further input events — otherwise a query typed (or
                // re-armed by Reload) on a small file that finished indexing
                // within the click frame would never be applied.
                if let Some(tab) = self.active_tab()
                    && let Some(wait) = tab.query.pending_debounce_wait()
                {
                    ctx.request_repaint_after(wait.max(Duration::from_millis(10)));
                }
            }
        }

        // Busy-repaint considers only the active tab (design D5): hidden
        // tabs' background work needs no repaint driving.
        let mut busy = false;
        if let Some(tab) = self.active_tab() {
            if !tab.file.index_done() {
                busy = true;
            }
            if let RowSource::Matched(handle) = &tab.rows
                && !handle.done.load(Ordering::Relaxed)
            {
                busy = true;
            }
        }
        if busy {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn enter_pressed_this_frame(&self, ctx: &egui::Context) -> bool {
        ctx.input(|i| i.key_pressed(egui::Key::Enter))
    }

    /// Clone the data the table needs for one frame.
    fn snapshot_view(&self) -> Option<TableView> {
        let tab = self.active_tab()?;
        Some(TableView {
            map: Arc::clone(&tab.file.map),
            index: Arc::clone(&tab.file.index),
            rows: tab.rows.clone(),
            columns: tab.file.visible_columns.clone(),
        })
    }

    /// Lines scanned so far by the active tab's query scan, if any.
    fn scan_progress(&self) -> Option<(usize, bool)> {
        match &self.active_tab()?.rows {
            RowSource::All => None,
            RowSource::Matched(handle) => Some((
                handle.scanned.load(Ordering::Relaxed),
                handle.done.load(Ordering::Relaxed),
            )),
        }
    }

    /// Pretty-printed text for the active tab's selected row, when a file is
    /// open and a row is selected (design: row selection / detail pane).
    fn selected_row_detail(&self) -> Option<String> {
        let tab = self.active_tab()?;
        row_detail(&tab.file, tab.selected_line?)
    }
}

impl eframe::App for LogAnalyzerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("log_analyzer");
                ui.separator();
                if ui.button("Open...").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file()
                {
                    // No extension filter (spec: log-file-access / Opening a
                    // log file): any picked file opens; line classification
                    // decides structure per line, independent of the name.
                    self.open_path(&path);
                }
                // Reload the current file from disk; inactive with no file
                // open (spec: log-file-access / Reloading the current file).
                if ui
                    .add_enabled(self.active_tab().is_some(), egui::Button::new("Reload"))
                    .clicked()
                {
                    self.reload_current();
                }
                // Favorite toggle for the open file (spec:
                // workspace-persistence / Managing favorites).
                let favorite = self
                    .current_file_key()
                    .is_some_and(|key| self.workspace.is_favorite(&key));
                let star = if favorite { "★" } else { "☆" };
                if ui
                    .add_enabled(self.active_tab().is_some(), egui::Button::new(star))
                    .clicked()
                {
                    self.toggle_favorite_current();
                }
                // Favorites list: click to open, x to remove (spec:
                // workspace-persistence / Managing favorites). Presented in
                // sorted order, not marking order.
                ui.menu_button("Favorites", |ui| {
                    let favorites = self.workspace.sorted_favorites();
                    if favorites.is_empty() {
                        ui.weak("no favorites yet");
                    }
                    for fav in favorites {
                        ui.horizontal(|ui| {
                            let path = PathBuf::from(&fav);
                            if ui.small_button(path.display().to_string()).clicked() {
                                self.open_path(&path);
                                ui.close();
                            }
                            if ui.small_button("x").clicked() {
                                self.remove_favorite(&fav);
                                ui.close();
                            }
                        });
                    }
                });
                ui.separator();
                let enter_now = self.enter_pressed_this_frame(ctx);
                if let Some(tab) = self.active_tab_mut() {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut tab.query.input)
                            .hint_text("query: level=ERROR timeout")
                            .desired_width(420.0),
                    );
                    if response.changed() {
                        tab.query.note_edit();
                    }
                    if response.lost_focus() && enter_now {
                        // `poll` on the next frame handles the forced apply via
                        // the global Enter check.
                    }
                } else {
                    ui.add_enabled(
                        false,
                        egui::TextEdit::singleline(&mut String::new())
                            .hint_text("query: level=ERROR timeout")
                            .desired_width(420.0),
                    );
                }
                let invalid = self.active_tab().and_then(|tab| tab.query.invalid.clone());
                let input_empty = self
                    .active_tab()
                    .is_none_or(|tab| tab.query.input.trim().is_empty());
                match &invalid {
                    Some(message) => {
                        ui.colored_label(egui::Color32::RED, format!("invalid query: {message}"));
                    }
                    None => {
                        if input_empty {
                            ui.weak("showing all rows");
                        }
                    }
                }
                if ui.button("Reset").clicked() {
                    if let Some(tab) = self.active_tab_mut() {
                        tab.query.reset();
                    }
                    self.reset_rows_to_all();
                }
                // Filter modifier for click-to-filter (spec:
                // modifier-click-filtering / Configurable filter modifier).
                let mut chosen = self.filter_modifier;
                egui::ComboBox::from_id_salt("filter_modifier")
                    .selected_text(self.filter_modifier.label())
                    .show_ui(ui, |ui| {
                        for option in FilterModifier::ALL {
                            ui.selectable_value(&mut chosen, option, option.label());
                        }
                    });
                if chosen != self.filter_modifier {
                    self.set_filter_modifier(chosen);
                }
                ui.separator();
                if let Some(tab) = self.active_tab() {
                    ui.label(tab.file.path.display().to_string());
                }
            });
        });

        // Tab strip: one entry per opened file (spec: log-tabs). Hidden
        // while no file is open. Labels are collected up front so the loop
        // can switch or close tabs through `&mut self`.
        if !self.tabs.is_empty() {
            let labels: Vec<(String, String)> = self
                .tabs
                .iter()
                .map(|tab| {
                    (
                        tab_label(&tab.file.path),
                        tab.file.path.display().to_string(),
                    )
                })
                .collect();
            egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (index, (label, full_path)) in labels.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let response = ui
                                .selectable_label(index == self.active, label.as_str())
                                .on_hover_text(full_path.as_str());
                            if response.clicked() {
                                self.active = index;
                            }
                            if ui.small_button("x").clicked() {
                                self.close_tab(index);
                            }
                        });
                    }
                });
            });
        }

        egui::SidePanel::right("inspector").show(ctx, |ui| {
            // Upper pane: column selection (spec: log-table-view).
            egui::TopBottomPanel::top("columns_pane")
                .resizable(true)
                .default_height(340.0)
                .min_height(140.0)
                .show_inside(ui, |ui| {
                    ui.heading("Columns");
                    ui.separator();
                    let Some(tab) = self.active_tab_mut() else {
                        ui.weak("no file open");
                        return;
                    };
                    let discovered: Vec<String> =
                        tab.file.fields.lock().unwrap().iter().cloned().collect();
                    let visible: Vec<String> = tab.file.visible_columns.clone();
                    // Field-name filter: narrows the list only; selection and
                    // the table are untouched (spec: log-table-view).
                    ui.add(
                        egui::TextEdit::singleline(&mut tab.column_filter)
                            .hint_text("filter fields")
                            .desired_width(ui.available_width()),
                    );
                    let column_filter = tab.column_filter.clone();
                    let shown: Vec<String> = discovered
                        .into_iter()
                        .filter(|name| field_matches_filter(name, &column_filter))
                        .collect();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for name in shown {
                            let mut on = visible.contains(&name);
                            if ui.checkbox(&mut on, name.as_str()).changed() {
                                self.set_column_visible(&name, on);
                            }
                        }
                    });
                });

            // Lower pane: pretty-printed contents of the selected row.
            let detail = self.selected_row_detail();
            let selected_line = self.active_tab().and_then(|tab| tab.selected_line);
            ui.horizontal(|ui| {
                ui.heading("Row detail");
                if let Some(line) = selected_line {
                    ui.weak(format!("line {}", line + 1));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(detail.is_some(), egui::Button::new("Copy"))
                        .clicked()
                        && let Some(text) = &detail
                    {
                        ui.ctx().copy_text(text.clone());
                    }
                });
            });
            ui.separator();
            match detail {
                Some(text) => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).monospace())
                                .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                    });
                }
                None => {
                    ui.weak("select a row to inspect its contents");
                }
            }
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::RED, error);
                    ui.separator();
                }
                if let Some(tab) = self.active_tab() {
                    let file = &tab.file;
                    if !file.index_done() {
                        ui.label(format!("indexing... {} lines", file.line_count()));
                    } else {
                        match self.scan_progress() {
                            Some((scanned, done)) if !done => {
                                ui.label(format!(
                                    "scanning... {scanned}/{} lines",
                                    file.line_count()
                                ));
                            }
                            _ => {
                                let shown = tab.rows.row_count(&file.index.read().unwrap());
                                ui.label(format!("{shown} / {} rows", file.line_count()));
                            }
                        }
                    }
                } else {
                    ui.weak("open a log file to begin");
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(view) = self.snapshot_view() else {
                ui.centered_and_justified(|ui| {
                    ui.weak("Open a log file to explore it.");
                });
                return;
            };
            let filter_modifier = self.filter_modifier;
            if let Some(tab) = self.active_tab_mut() {
                render_table(
                    ui,
                    &view,
                    &mut tab.cache,
                    &mut tab.selected_line,
                    filter_modifier,
                    &mut tab.query,
                );
            }
        });
    }
}

/// Render the virtualized table; `selected` is the selected row's line
/// number, toggled by clicking rows without `filter_modifier` held. With the
/// modifier held, a structured cell click instead replaces the contents of
/// `query_ui` with the cell's filter term (spec: modifier-click-filtering).
fn render_table(
    ui: &mut egui::Ui,
    view: &TableView,
    cache: &mut ParseCache,
    selected: &mut Option<usize>,
    filter_modifier: FilterModifier,
    query_ui: &mut QueryUi,
) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let row_count = view
        .index
        .read()
        .map(|index| view.rows.row_count(&index))
        .unwrap_or(0);

    let mut table = egui_extras::TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .vscroll(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

    // Ensure at least one column exists so raw rows always have somewhere to
    // render their text.
    if view.columns.is_empty() {
        table = table.column(Column::remainder().resizable(false));
    } else {
        for (i, _) in view.columns.iter().enumerate() {
            let column = if i + 1 == view.columns.len() {
                Column::remainder().resizable(true).clip(true)
            } else {
                Column::initial(160.0).resizable(true).clip(true)
            };
            table = table.column(column);
        }
    }

    let columns = view.columns.clone();
    table
        .header(row_height, |mut header| {
            if columns.is_empty() {
                header.col(|ui| {
                    ui.strong("(raw)");
                });
            } else {
                for name in &columns {
                    header.col(|ui| {
                        ui.strong(name);
                    });
                }
            }
        })
        .body(|body| {
            body.rows(row_height, row_count, |mut row| {
                let row_index = row.index();
                let index_guard = match view.index.read() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                let Some(line_no) = view.rows.line_number(row_index, &index_guard) else {
                    return;
                };
                let Some(slice) = index_guard.line_slice(&view.map, line_no) else {
                    return;
                };
                let entry = cache.get_or_compute(line_no, || log_file::parse_object(slice));
                let tint = level_tint(entry.as_deref());
                let text = String::from_utf8_lossy(slice);
                let text = truncate_chars(&text, MAX_CELL_CHARS);

                if columns.is_empty() {
                    row.col(|ui| {
                        let cell_rect = ui.max_rect();
                        if *selected == Some(line_no) {
                            ui.painter().rect_filled(cell_rect, 0.0, SELECTED_ROW_TINT);
                        }
                        // The hit overlay must be created after the cell
                        // content: the topmost widget wins hit-testing, and an
                        // overflowing truncated label otherwise shadows clicks
                        // on filled cells. The elided-text tooltip is also
                        // suppressed: its popup layer would sit over the
                        // pointer and block row clicks.
                        ui.add(
                            egui::Label::new(egui::RichText::new(text.clone()).monospace())
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .show_tooltip_when_elided(false),
                        );
                        let hit = ui
                            .interact(
                                cell_rect,
                                egui::Id::new(("row-hit", line_no, 0usize)),
                                egui::Sense::click(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        // Raw lines are not filterable (spec:
                        // modifier-click-filtering / Ineligible cells do
                        // nothing): with the modifier held the click is a
                        // no-op, without it the usual selection toggle runs.
                        let modifier_held =
                            filter_modifier.matches_modifier(ui.input(|i| i.modifiers));
                        if hit.clicked() && !modifier_held {
                            apply_row_click(selected, line_no);
                        }
                    });
                    return;
                }

                for (i, column) in columns.iter().enumerate() {
                    let is_first = i == 0;
                    row.col(|ui| {
                        let cell_rect = ui.max_rect();
                        // Severity tint under the cell content (spec:
                        // log-table-view). Raw rows carry no tint.
                        if let Some(tint) = tint {
                            ui.painter().rect_filled(cell_rect, 0.0, tint);
                        }
                        if *selected == Some(line_no) {
                            ui.painter().rect_filled(cell_rect, 0.0, SELECTED_ROW_TINT);
                        }
                        // Hit overlay after the content: see the raw-branch
                        // comment above (topmost widget wins hit-testing).
                        match &entry {
                            Some(obj) => {
                                let cell = obj
                                    .get(column)
                                    .map(crate::query::field_value_as_text)
                                    .unwrap_or_default();
                                ui.add(
                                    egui::Label::new(truncate_chars(&cell, MAX_CELL_CHARS))
                                        .wrap_mode(egui::TextWrapMode::Truncate)
                                        .show_tooltip_when_elided(false),
                                );
                            }
                            None => {
                                // Raw line: full text in the first visible
                                // column, dimmed (design D7).
                                if is_first {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(text.clone()).weak().monospace(),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Truncate)
                                        .show_tooltip_when_elided(false),
                                    );
                                }
                            }
                        }
                        let hit = ui
                            .interact(
                                cell_rect,
                                egui::Id::new(("row-hit", line_no, i)),
                                egui::Sense::click(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit.clicked() {
                            let modifier_held =
                                filter_modifier.matches_modifier(ui.input(|i| i.modifiers));
                            if !modifier_held {
                                apply_row_click(selected, line_no);
                            } else if let Some(term) = entry.as_deref().and_then(|obj| {
                                obj.get(column)
                                    .map(query::field_value_as_text)
                                    .and_then(|value| filter_term(column, &value))
                            }) {
                                query_ui.replace_filter_term(&term);
                            }
                            // A modifier-click on an ineligible cell (raw
                            // row, missing value, unqueryable name,
                            // unrepresentable value) does nothing: neither
                            // the query nor the selection changes (spec:
                            // modifier-click-filtering).
                        }
                    });
                }
            });
        });
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

/// Click behavior of row selection: clicking the selected row deselects it,
/// clicking any other row selects that one.
fn apply_row_click(selected: &mut Option<usize>, line_no: usize) {
    *selected = if *selected == Some(line_no) {
        None
    } else {
        Some(line_no)
    };
}

/// Query term that filters for a clicked cell's value (spec:
/// modifier-click-filtering / Filter term from a modifier-click). Built from
/// the entry value — the exact text field-equality compares, so the clicked
/// row always matches its own term — never from the truncated display text.
/// Single quotes by default, double quotes when the value itself contains a
/// single quote; `None` when the value contains both quote kinds or a line
/// break (unrepresentable), or the field name is not a valid query field name
/// (an unqueryable JSON key would poison the whole query into "invalid").
fn filter_term(field: &str, value: &str) -> Option<String> {
    if !query::is_valid_field_name(field) {
        return None;
    }
    let has_single = value.contains('\'');
    let has_double = value.contains('"');
    if (has_single && has_double) || value.contains(['\n', '\r']) {
        return None;
    }
    let quote = if has_single { '"' } else { '\'' };
    Some(format!("{field}={quote}{value}{quote}"))
}

/// Detail-pane text for one file line: pretty-printed JSON for structured
/// entries, the lossy raw text otherwise, truncated beyond
/// [`DETAIL_MAX_CHARS`].
fn row_detail_text(slice: &[u8]) -> String {
    let pretty =
        log_file::parse_object(slice).and_then(|obj| serde_json::to_string_pretty(&obj).ok());
    let text = pretty.unwrap_or_else(|| String::from_utf8_lossy(slice).into_owned());
    truncate_chars(&text, DETAIL_MAX_CHARS)
}

/// Detail-pane text for a line of an open file, read on demand through the
/// shared index.
fn row_detail(file: &LoadedFile, line: usize) -> Option<String> {
    let index_guard = file.index.read().ok()?;
    let slice = index_guard.line_slice(&file.map, line)?;
    Some(row_detail_text(slice))
}

/// Background tint for a row based on its structured entry's `level` field,
/// compared case-insensitively (spec: log-table-view). Raw rows, entries
/// without a `level` field, and any non-error/warning level keep the default
/// background.
fn level_tint(entry: Option<&serde_json::Map<String, Value>>) -> Option<egui::Color32> {
    let level = entry?.get("level")?.as_str()?;
    match level.to_ascii_lowercase().as_str() {
        "error" => Some(ERROR_ROW_TINT),
        "warn" | "warning" => Some(WARN_ROW_TINT),
        _ => None,
    }
}

/// True when a field name passes the Columns panel filter: an empty (or
/// all-whitespace) filter passes everything, otherwise a case-insensitive
/// substring match on the name (spec: log-table-view / Filtering the field
/// list). Filtering never changes which columns are selected.
fn field_matches_filter(name: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

/// Tab-strip label for a file: its name, falling back to the full path when
/// the path has no file name component (spec: log-tabs).
fn tab_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicUsize;

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("log_analyzer_test_{name}"));
        let mut file = File::create(&path).unwrap();
        file.write_all(contents).unwrap();
        path
    }

    fn fixture_index(bytes: &[u8]) -> IndexData {
        IndexData::build(bytes)
    }

    // ---- 4.2: query scan worker ----

    #[test]
    fn query_scan_collects_expected_line_numbers() {
        // Note: a substring term matches the raw line text, so line 3's
        // message deliberately avoids the word "timeout".
        let bytes = b"{\"level\": \"INFO\"}\n\
                      noise line with timeout\n\
                      {\"level\": \"ERROR\", \"message\": \"timeout in upstream\"}\n\
                      {\"level\": \"ERROR\", \"message\": \"no delays here\"}\n\
                      PANIC: timeout\n";
        let path = temp_file("scan_fixture.log", bytes);
        let file = LoadedFile::open(&path).unwrap();
        let index = fixture_index(bytes);
        let query = query::parse("level=ERROR timeout").unwrap();
        let out = RwLock::new(Vec::new());
        let fields = Mutex::new(BTreeSet::new());
        run_query_scan(
            &file.map,
            &index,
            &query,
            &out,
            &fields,
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        assert_eq!(*out.write().unwrap(), vec![2]);

        // Substring-only query also hits raw lines.
        let substring = query::parse("timeout").unwrap();
        let out2 = RwLock::new(Vec::new());
        run_query_scan(
            &file.map,
            &index,
            &substring,
            &out2,
            &fields,
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        assert_eq!(*out2.write().unwrap(), vec![1, 2, 4]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn query_scan_respects_cancel_and_piggybacks_fields() {
        let bytes = b"{\"level\": \"INFO\", \"service\": \"a\"}\n{\"level\": \"ERROR\"}\n";
        let path = temp_file("scan_cancel_fixture.log", bytes);
        let file = LoadedFile::open(&path).unwrap();
        let index = fixture_index(bytes);
        let q = query::parse("level=ERROR").unwrap();
        let out = RwLock::new(Vec::new());
        let fields = Mutex::new(BTreeSet::new());

        // Pre-cancelled: nothing is collected.
        run_query_scan(
            &file.map,
            &index,
            &q,
            &out,
            &fields,
            &AtomicBool::new(true),
            &AtomicUsize::new(0),
        );
        assert!(out.write().unwrap().is_empty());

        // Un-cancelled run collects fields as a side effect (design D6).
        run_query_scan(
            &file.map,
            &index,
            &q,
            &out,
            &fields,
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        assert_eq!(*out.write().unwrap(), vec![1]);
        let fields = fields.lock().unwrap();
        assert!(fields.contains("level") && fields.contains("service"));
        let _ = std::fs::remove_file(&path);
    }

    // ---- 4.3: open-failure retention ----

    #[test]
    fn open_failure_keeps_previous_file() {
        let bytes = b"{\"a\": 1}\n";
        let path = temp_file("retention.log", bytes);
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.on_open_result_inner(Ok(LoadedFile::open(&path).unwrap()));
        let first_path = app.active_tab().unwrap().file.path.clone();

        app.on_open_result_inner(Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "denied",
        )));
        assert_eq!(app.active_tab().unwrap().file.path, first_path);
        assert!(app.error.is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_success_replaces_state_and_sets_defaults() {
        let bytes = b"{\"@timestamp\": \"t\", \"level\": \"INFO\", \"message\": \"m\"}\n";
        let path = temp_file("defaults.log", bytes);
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.on_open_result_inner(Ok(LoadedFile::open(&path).unwrap()));
        // Defaults cannot be picked at open time; they are applied once field
        // discovery has produced names (mirrors the flow in `poll`).
        {
            let tab = app.active_tab_mut().unwrap();
            let mut fields = tab.file.fields.lock().unwrap();
            fields.insert("@timestamp".to_owned());
            fields.insert("level".to_owned());
            fields.insert("message".to_owned());
        }
        app.maybe_apply_columns();
        assert_eq!(
            app.active_tab().unwrap().file.visible_columns,
            vec!["@timestamp", "level", "message"]
        );
        // Later frames (and user edits) are not clobbered: defaults apply once.
        app.maybe_apply_columns();
        assert_eq!(
            app.active_tab().unwrap().file.visible_columns,
            vec!["@timestamp", "level", "message"]
        );
        let _ = std::fs::remove_file(&path);
    }

    // ---- 4.2: tabbed open flow (focus-or-push) ----

    #[test]
    fn open_focuses_existing_tab_and_preserves_context() {
        let bytes = b"{\"level\": \"INFO\"}\n";
        let path = temp_file("focus_existing.log", bytes);
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.open_path(&path);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.active_tab().unwrap().file.index_done() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Distinctive context on the tab, plus a handle on its shared index
        // so the no-re-read guarantee is observable.
        let tab = app.active_tab_mut().unwrap();
        tab.query.input = "level=ERROR".into();
        tab.column_filter = "req".into();
        tab.selected_line = Some(0);
        let index_arc = Arc::clone(&tab.file.index);

        // Re-opening the same path focuses the existing tab: one tab, the
        // same read (shared index identity), context untouched.
        app.open_path(&path);
        assert_eq!(app.tabs.len(), 1, "no duplicate tab");
        assert_eq!(app.active, 0);
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.file.path, path);
        assert!(
            Arc::ptr_eq(&index_arc, &tab.file.index),
            "the file was not re-read"
        );
        assert_eq!(tab.query.input, "level=ERROR");
        assert_eq!(tab.column_filter, "req");
        assert_eq!(tab.selected_line, Some(0));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn opening_second_file_keeps_first_tab() {
        let a_path = temp_file("second_a.log", b"{\"level\": \"INFO\"}\n");
        let b_path = temp_file("second_b.log", b"{\"level\": \"ERROR\"}\n");
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.open_path(&a_path);
        app.active_tab_mut().unwrap().query.input = "level=INFO".into();

        // Opening a second file pushes and activates a new tab.
        app.open_path(&b_path);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 1);
        assert_eq!(app.active_tab().unwrap().file.path, b_path);

        // The first tab keeps its file and context, and the second open did
        // not cancel its background work.
        let first = &app.tabs[0];
        assert_eq!(first.file.path, a_path);
        assert_eq!(first.query.input, "level=INFO");
        assert!(!first.file.open_cancel.load(Ordering::Relaxed));

        // Reopening the first file focuses its tab instead of duplicating it.
        app.open_path(&a_path);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 0);
        let _ = std::fs::remove_file(&a_path);
        let _ = std::fs::remove_file(&b_path);
    }

    #[test]
    fn close_tab_cancels_removed_tab_and_clamps_active() {
        let a = temp_file("tabs_a.log", b"{\"level\": \"INFO\"}\n");
        let b = temp_file("tabs_b.log", b"{\"level\": \"INFO\"}\n");
        let c = temp_file("tabs_c.log", b"{\"level\": \"INFO\"}\n");
        let d = temp_file("tabs_d.log", b"{\"level\": \"INFO\"}\n");
        let e = temp_file("tabs_e.log", b"{\"level\": \"INFO\"}\n");
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.open_path(&a);
        app.open_path(&b);
        app.open_path(&c);
        app.active = 1; // B active

        // Closing an earlier inactive tab shifts the strip; B stays in view.
        app.close_tab(0);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 0);
        assert_eq!(app.active_tab().unwrap().file.path, b);

        // Closing a tab right of the active one leaves focus alone and
        // cancels the closed tab's background work.
        let c_cancel = Arc::clone(&app.tabs[1].file.open_cancel);
        app.close_tab(1);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active, 0);
        assert!(c_cancel.load(Ordering::Relaxed));

        // Closing the last remaining tab leaves the empty state.
        let b_cancel = Arc::clone(&app.tabs[0].file.open_cancel);
        app.close_tab(0);
        assert!(app.tabs.is_empty());
        assert!(app.active_tab().is_none());
        assert!(b_cancel.load(Ordering::Relaxed));

        // Closing the active tab with a left neighbor focuses that neighbor.
        let app2 = &mut LogAnalyzerApp::with_workspace(None);
        app2.open_path(&d);
        app2.open_path(&e);
        app2.active = 1;
        app2.close_tab(1);
        assert_eq!(app2.active, 0);
        assert_eq!(app2.active_tab().unwrap().file.path, d);

        for path in [&a, &b, &c, &d, &e] {
            let _ = std::fs::remove_file(path);
        }
    }

    // ---- 4.1: open flow with background indexing + discovery ----

    #[test]
    fn open_path_indexes_fixture_and_discovers_fields() {
        let path = std::path::Path::new("log_examples/first.log");
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.open_path(path);
        let file = &app.active_tab().expect("file loaded").file;

        // Wait for the background index build (progressive availability:
        // line_count grows before `done` flips).
        let deadline = Instant::now() + Duration::from_secs(10);
        while !file.index_done() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(file.index_done(), "indexing finished");
        let lines = file.line_count();
        assert!(lines >= 300, "fixture has ~306 lines, got {lines}");

        // Wait for field discovery, then defaults.
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.active_tab().unwrap().file.fields.lock().unwrap().is_empty()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        app.maybe_apply_columns();
        let file = &app.active_tab().unwrap().file;
        let fields = file.fields.lock().unwrap();
        for expected in ["@timestamp", "level", "message", "durationMs"] {
            assert!(fields.contains(expected), "missing field {expected}");
        }
        assert_eq!(file.visible_columns, vec!["@timestamp", "level", "message"]);
        // First and last line are readable through the shared index.
        let index = file.index.read().unwrap();
        let first = index.line_slice(&file.map, 0).unwrap();
        assert!(std::str::from_utf8(first).is_ok());
    }

    // ---- 6.1/6.2: query input state machine ----

    #[test]
    fn query_ui_debounce_enter_invalid_and_clear() {
        let mut q = QueryUi::new();
        // Nothing typed: nothing to apply.
        assert!(q.take_pending_apply(false).is_none());

        // Edit within the debounce window: no apply yet.
        q.input = "field=".into();
        q.note_edit();
        assert!(q.take_pending_apply(false).is_none());

        // Enter forces an apply: invalid feedback is surfaced.
        let applied = q.take_pending_apply(true).unwrap();
        assert!(applied.is_err());
        assert!(q.invalid.is_some());

        // Invalid query means all rows stay visible (no scan was started);
        // clearing the input restores the all-rows state cleanly.
        q.input.clear();
        q.note_edit();
        let applied = q.take_pending_apply(true).unwrap();
        assert!(applied.unwrap().is_empty());
        assert!(q.invalid.is_none());

        // Debounce path: after the interval elapses, the edit applies itself.
        q.input = "level=ERROR".into();
        q.note_edit();
        std::thread::sleep(QUERY_DEBOUNCE + Duration::from_millis(20));
        let applied = q.take_pending_apply(false).unwrap();
        assert!(!applied.unwrap().is_empty());
    }

    #[test]
    fn query_ui_pending_debounce_wait_tracks_pending_edits() {
        let mut q = QueryUi::new();
        assert!(
            q.pending_debounce_wait().is_none(),
            "no apply owed when nothing was typed"
        );

        // A fresh edit is pending and reports remaining debounce time.
        q.input = "level=ERROR".into();
        q.note_edit();
        let wait = q.pending_debounce_wait().expect("edit pending");
        assert!(wait > Duration::ZERO && wait <= QUERY_DEBOUNCE);

        // Once the debounce elapses the wait reaches zero (repaint now).
        std::thread::sleep(QUERY_DEBOUNCE + Duration::from_millis(20));
        assert_eq!(q.pending_debounce_wait(), Some(Duration::ZERO));

        // After applying, nothing is owed.
        q.applied_input = q.input.clone();
        assert!(q.pending_debounce_wait().is_none());
    }

    // ---- 2.1: new parse-error kinds route through the invalid path ----

    #[test]
    fn query_ui_surfaces_new_invalid_kinds() {
        for (input, needle) in [
            ("(level=ERROR timeout", "missing closing"),
            ("level=ERROR) timeout", "unbalanced"),
            ("requestId='abc", "unterminated quote"),
            ("level=ERROR or", "expected a term"),
        ] {
            let mut q = QueryUi::new();
            q.input = input.into();
            q.note_edit();
            let applied = q.take_pending_apply(true).expect("an apply decision");
            let msg = applied.expect_err("must be invalid");
            assert!(msg.contains(needle), "{input}: {msg}");
            assert!(q.invalid.is_some(), "{input} must set the invalid flag");
        }
    }

    // ---- 2.2: one-action reset ----

    #[test]
    fn query_ui_reset_clears_input_and_invalid() {
        let mut q = QueryUi::new();
        // A filtered state…
        q.input = "level=ERROR".into();
        q.note_edit();
        assert!(q.take_pending_apply(true).unwrap().is_ok());
        // …then an invalid state.
        q.input = "field=".into();
        q.note_edit();
        assert!(q.take_pending_apply(true).unwrap().is_err());
        assert!(q.invalid.is_some());

        // One reset action clears the input and the invalid indication, and
        // leaves nothing pending to apply (the app then restores all rows).
        q.reset();
        assert!(q.input.is_empty());
        assert!(q.invalid.is_none());
        assert!(q.take_pending_apply(true).is_none());
    }

    // ---- 3.2: severity tint classifier ----

    /// Build a structured entry from key/value pairs.
    fn map_with(pairs: &[(&str, Value)]) -> serde_json::Map<String, Value> {
        pairs
            .iter()
            .fold(serde_json::Map::new(), |mut map, (key, value)| {
                map.insert((*key).to_owned(), value.clone());
                map
            })
    }

    #[test]
    fn level_tint_classifies_case_insensitively() {
        use serde_json::json;
        let lvl = |v: Value| map_with(&[("level", v)]);
        assert_eq!(level_tint(Some(&lvl(json!("ERROR")))), Some(ERROR_ROW_TINT));
        assert_eq!(level_tint(Some(&lvl(json!("Error")))), Some(ERROR_ROW_TINT));
        assert_eq!(level_tint(Some(&lvl(json!("WARN")))), Some(WARN_ROW_TINT));
        assert_eq!(
            level_tint(Some(&lvl(json!("Warning")))),
            Some(WARN_ROW_TINT)
        );
        assert_eq!(
            level_tint(Some(&lvl(json!("warning")))),
            Some(WARN_ROW_TINT)
        );
        assert_eq!(level_tint(Some(&lvl(json!("INFO")))), None);
        assert_eq!(level_tint(Some(&lvl(json!("debug")))), None);
        assert_eq!(level_tint(Some(&lvl(json!(3)))), None, "non-string level");
        assert_eq!(
            level_tint(Some(&map_with(&[("other", json!("x"))]))),
            None,
            "entry without a level field"
        );
        assert_eq!(level_tint(None), None, "raw row never tints");
    }

    // ---- 4.1/4.2: columns panel filter ----

    #[test]
    fn field_filter_matches_case_insensitively() {
        // Empty and whitespace-only filters pass everything through.
        assert!(field_matches_filter("requestId", ""));
        assert!(field_matches_filter("requestId", "   "));
        assert!(field_matches_filter("requestId", "req"));
        assert!(field_matches_filter("requestId", "UEST"));
        assert!(field_matches_filter("requestId", "id"));
        assert!(!field_matches_filter("requestId", "zzz"));
        assert!(!field_matches_filter("level", "message"));
    }

    #[test]
    fn column_filter_hides_names_but_keeps_selection() {
        let bytes = b"{\"level\": \"INFO\", \"message\": \"m\"}\n";
        let path = temp_file("column_filter.log", bytes);
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.on_open_result_inner(Ok(LoadedFile::open(&path).unwrap()));
        let tab = app.active_tab_mut().unwrap();
        tab.file.visible_columns = vec!["level".into(), "message".into()];
        tab.column_filter = "req".into();

        // What the panel would render now: only the matching names…
        let discovered = ["level", "message", "requestId"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let shown: Vec<&String> = discovered
            .iter()
            .filter(|name| field_matches_filter(name, &tab.column_filter))
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0], "requestId");

        // …while the hidden-but-selected columns stay applied to the table.
        assert_eq!(
            app.active_tab().unwrap().file.visible_columns,
            vec!["level", "message"]
        );
        let _ = std::fs::remove_file(&path);
    }

    // ---- 5.2/5.3/5.4: workspace wiring ----

    #[test]
    fn workspace_persists_across_app_instances() {
        let ws_path = std::env::temp_dir().join("log_analyzer_test_ws_roundtrip.json");
        let _ = std::fs::remove_file(&ws_path);
        let bytes = b"{\"level\": \"INFO\"}\n";
        let log_path = temp_file("ws_roundtrip.log", bytes);
        let key = persistence::path_key(&log_path);

        let app = &mut LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        app.on_open_result_inner(Ok(LoadedFile::open(&log_path).unwrap()));
        assert!(app.toggle_favorite_current().unwrap());
        // Write-through: the workspace file exists after the mutation.
        assert!(ws_path.exists());

        // A fresh instance loads the same workspace and sees the favorite.
        let app2 = LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        assert!(app2.workspace.is_favorite(&key));

        let _ = std::fs::remove_file(&ws_path);
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn favorite_toggle_and_removal_persist() {
        let ws_path = std::env::temp_dir().join("log_analyzer_test_ws_favorites.json");
        let _ = std::fs::remove_file(&ws_path);
        let bytes = b"{\"level\": \"INFO\"}\n";
        let log_path = temp_file("ws_favorites.log", bytes);
        let key = persistence::path_key(&log_path);

        let app = &mut LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        app.on_open_result_inner(Ok(LoadedFile::open(&log_path).unwrap()));

        // Toggle on: marked and persisted.
        assert!(app.toggle_favorite_current().unwrap());
        assert!(Workspace::load(&ws_path).is_favorite(&key));

        // Removing the open file's favorite unmarks the star.
        app.remove_favorite(&key);
        assert!(!app.workspace.is_favorite(&key));
        assert!(!Workspace::load(&ws_path).is_favorite(&key));
        // Toggling again re-adds it.
        assert!(app.toggle_favorite_current().unwrap());
        assert_eq!(app.current_file_key().as_deref(), Some(key.as_str()));

        let _ = std::fs::remove_file(&ws_path);
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn filter_modifier_change_persists() {
        let ws_path = std::env::temp_dir().join("log_analyzer_test_ws_filter_modifier.json");
        let _ = std::fs::remove_file(&ws_path);

        // Defaults to Ctrl; a change is written through immediately.
        let app = &mut LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        assert_eq!(app.filter_modifier, FilterModifier::Ctrl);
        app.set_filter_modifier(FilterModifier::Alt);
        assert_eq!(app.filter_modifier, FilterModifier::Alt);
        assert!(ws_path.exists());

        // A fresh instance restores the choice (spec: the choice persists).
        let app2 = LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        assert_eq!(app2.filter_modifier, FilterModifier::Alt);

        let _ = std::fs::remove_file(&ws_path);
    }

    #[test]
    fn saved_columns_restored_over_defaults_and_write_through() {
        let ws_path = std::env::temp_dir().join("log_analyzer_test_ws_columns.json");
        let _ = std::fs::remove_file(&ws_path);
        let bytes =
            b"{\"@timestamp\": \"t\", \"level\": \"INFO\", \"message\": \"m\", \"extra\": 1}\n";
        let log_path = temp_file("ws_columns.log", bytes);
        let key = persistence::path_key(&log_path);
        fn discover(app: &mut LogAnalyzerApp) {
            let tab = app.active_tab_mut().unwrap();
            let mut fields = tab.file.fields.lock().unwrap();
            for name in ["@timestamp", "level", "message", "extra"] {
                fields.insert(name.to_owned());
            }
        }

        // Instance 1: defaults apply at discovery, then a user change is
        // written through to the workspace.
        let app = &mut LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        app.on_open_result_inner(Ok(LoadedFile::open(&log_path).unwrap()));
        discover(app);
        app.maybe_apply_columns();
        assert_eq!(
            app.active_tab().unwrap().file.visible_columns,
            vec!["@timestamp", "level", "message"]
        );
        app.set_column_visible("message", false);
        assert_eq!(
            app.active_tab().unwrap().file.visible_columns,
            vec!["@timestamp", "level"]
        );
        assert_eq!(
            Workspace::load(&ws_path).columns_for(&key).unwrap(),
            &vec!["@timestamp".to_owned(), "level".to_owned()]
        );

        // Instance 2: reopening restores the saved selection over defaults.
        let app2 = &mut LogAnalyzerApp::with_workspace(Some(ws_path.clone()));
        app2.on_open_result_inner(Ok(LoadedFile::open(&log_path).unwrap()));
        discover(app2);
        app2.maybe_apply_columns();
        assert_eq!(
            app2.active_tab().unwrap().file.visible_columns,
            vec!["@timestamp", "level"]
        );

        let _ = std::fs::remove_file(&ws_path);
        let _ = std::fs::remove_file(&log_path);
    }

    // ---- 6.1/6.2: reload current file ----

    /// Block until the active tab's background scan finishes (tests run scans
    /// directly, without the egui poll loop).
    fn wait_scan_done(app: &LogAnalyzerApp) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match &app.active_tab().expect("active tab").rows {
                RowSource::Matched(handle) if handle.done.load(Ordering::Relaxed) => return,
                RowSource::Matched(_) => {}
                RowSource::All => panic!("expected a running query scan"),
            }
            assert!(Instant::now() < deadline, "scan did not finish in time");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn reload_picks_up_appended_lines_and_keeps_query() {
        let log_path = std::env::temp_dir().join("log_analyzer_test_reload.log");
        std::fs::write(&log_path, b"{\"level\": \"INFO\"}\n").unwrap();
        let app = &mut LogAnalyzerApp::with_workspace(None);

        app.open_path(&log_path);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.active_tab().unwrap().file.index_done() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.active_tab().unwrap().file.index_done());
        assert_eq!(app.active_tab().unwrap().file.line_count(), 1);

        // Discover fields and save a deliberately empty column selection, so
        // it can be told apart from the default selection (which would be
        // ["level"] here).
        app.active_tab_mut()
            .unwrap()
            .file
            .fields
            .lock()
            .unwrap()
            .insert("level".to_owned());
        app.maybe_apply_columns();
        assert_eq!(
            app.active_tab().unwrap().file.visible_columns,
            vec!["level"]
        );
        app.set_column_visible("level", false);

        // An active query filters the loaded rows.
        let tab = app.active_tab_mut().unwrap();
        tab.query.input = "level=ERROR".into();
        tab.query.note_edit();
        let q = tab.query.take_pending_apply(true).unwrap().unwrap();
        app.start_query_scan(q);
        wait_scan_done(app);
        let index_guard = app.active_tab().unwrap().file.index.read().unwrap();
        assert_eq!(
            app.active_tab().unwrap().rows.row_count(&index_guard),
            0,
            "no ERROR rows yet"
        );
        drop(index_guard);

        // Append to the file behind the app's back, then reload.
        let mut handle = std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap();
        handle.write_all(b"{\"level\": \"ERROR\"}\n").unwrap();
        drop(handle);
        app.reload_current();

        // The query text is preserved and re-armed so `poll` re-applies it
        // once indexing finishes.
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.query.input, "level=ERROR");
        assert!(tab.query.applied_input.is_empty(), "query re-armed");
        assert!(tab.query.last_edit.is_some());

        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.active_tab().unwrap().file.index_done() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.active_tab().unwrap().file.index_done());
        assert_eq!(
            app.active_tab().unwrap().file.line_count(),
            2,
            "reload picks up appended content"
        );

        // Field discovery re-runs on the reloaded file; once fields appear,
        // `poll` restores the saved selection instead of the defaults.
        let deadline = Instant::now() + Duration::from_secs(10);
        while app
            .active_tab()
            .unwrap()
            .file
            .fields
            .lock()
            .unwrap()
            .is_empty()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        app.maybe_apply_columns();
        assert!(
            app.active_tab().unwrap().file.visible_columns.is_empty(),
            "saved (empty) column selection restored over defaults"
        );

        // The step `poll` performs after re-arming: re-apply the preserved
        // query against the reloaded rows.
        let q = query::parse(&app.active_tab().unwrap().query.input).unwrap();
        app.start_query_scan(q);
        wait_scan_done(app);
        let index_guard = app.active_tab().unwrap().file.index.read().unwrap();
        assert_eq!(
            app.active_tab().unwrap().rows.row_count(&index_guard),
            1,
            "query still filters the reloaded rows"
        );
        drop(index_guard);
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn reload_without_file_is_noop() {
        let app = &mut LogAnalyzerApp::with_workspace(None);
        app.reload_current();
        assert!(app.active_tab().is_none());
        assert!(app.error.is_none());
    }

    // ---- 5.1: row source mapping ----

    #[test]
    fn row_source_maps_rows_to_line_numbers() {
        let index = fixture_index(b"a\nbb\nccc");
        // All source: identity mapping.
        let all = RowSource::All;
        assert_eq!(all.row_count(&index), 3);
        assert_eq!(all.line_number(0, &index), Some(0));
        assert_eq!(all.line_number(2, &index), Some(2));
        assert_eq!(all.line_number(3, &index), None);

        // Matched source: maps through the collected line numbers.
        let handle = ScanHandle::new();
        handle.line_numbers.write().unwrap().extend([0, 2]);
        let matched = RowSource::Matched(handle);
        assert_eq!(matched.row_count(&index), 2);
        assert_eq!(matched.line_number(0, &index), Some(0));
        assert_eq!(matched.line_number(1, &index), Some(2));
        assert_eq!(matched.line_number(2, &index), None);
    }

    // ---- row selection + detail pane ----

    #[test]
    fn row_click_toggles_and_moves_selection() {
        let mut selected: Option<usize> = None;
        apply_row_click(&mut selected, 3);
        assert_eq!(selected, Some(3));
        apply_row_click(&mut selected, 3);
        assert_eq!(selected, None, "clicking the selected row deselects it");
        apply_row_click(&mut selected, 5);
        apply_row_click(&mut selected, 7);
        assert_eq!(selected, Some(7), "selection moves to the clicked row");
    }

    #[test]
    fn filter_term_formats_and_rejects_values() {
        // Plain values are single-quoted.
        assert_eq!(
            filter_term("requestId", "abc-123").as_deref(),
            Some("requestId='abc-123'")
        );
        // A value containing a single quote switches to double quotes.
        assert_eq!(
            filter_term("note", "it's broken").as_deref(),
            Some("note=\"it's broken\"")
        );
        // A value containing a double quote stays single-quoted.
        assert_eq!(
            filter_term("note", "say \"hi\"").as_deref(),
            Some("note='say \"hi\"'")
        );
        // Unrepresentable values yield no term at all.
        assert_eq!(filter_term("note", "it's \"both\""), None);
        assert_eq!(filter_term("note", "two\nlines"), None);
        assert_eq!(filter_term("note", "two\rlines"), None);
        // Invalid field names yield no term (would poison the query).
        assert_eq!(filter_term("user name", "x"), None);
        assert_eq!(filter_term("1abc", "x"), None);
        assert_eq!(filter_term("", "x"), None);

        // The clicked row itself always matches its own generated term.
        for (field, value) in [
            ("requestId", "abc-123"),
            ("note", "it's broken"),
            ("count", "2758"),
        ] {
            let term = filter_term(field, value).expect("representable value");
            let q = query::parse(&term).unwrap();
            let row = map_with(&[(field, Value::from(value))]);
            assert!(q.matches("x", Some(&row)), "{term} must match its own row");
        }
    }

    #[test]
    fn replace_filter_term_rewrites_input() {
        // Mirrors `poll`: the forced flag is taken and OR'd into the force
        // argument of `take_pending_apply`.
        let take_force = |q: &mut QueryUi| std::mem::take(&mut q.force_next_apply);

        // An empty input becomes exactly the term.
        let mut q = QueryUi::new();
        q.replace_filter_term("requestId='abc-123'");
        assert_eq!(q.input, "requestId='abc-123'");
        let force = take_force(&mut q);
        assert!(force, "the replacement must force the apply");
        let applied = q.take_pending_apply(force).unwrap();
        assert_eq!(
            applied.unwrap(),
            query::parse("requestId='abc-123'").unwrap()
        );

        // A non-empty input is rewritten: the previous query text is
        // discarded and the term applies alone, without the debounce wait.
        let mut q = QueryUi::new();
        q.input = "level=ERROR service='auth'".into();
        q.note_edit();
        assert!(
            q.take_pending_apply(false).is_none(),
            "a fresh edit stays inside the debounce window"
        );
        q.replace_filter_term("requestId='abc-123'");
        assert_eq!(q.input, "requestId='abc-123'");
        let force = take_force(&mut q);
        let applied = q.take_pending_apply(force).unwrap();
        assert_eq!(
            applied.unwrap(),
            query::parse("requestId='abc-123'").unwrap()
        );

        // The flag is one-shot: without it a fresh edit is debounced again.
        q.input = "timeout".into();
        q.note_edit();
        assert!(!take_force(&mut q));
        assert!(q.take_pending_apply(false).is_none());
    }

    #[test]
    fn row_detail_pretty_prints_json_and_keeps_raw_text() {
        let pretty = row_detail_text(b"{\"level\":\"INFO\",\"message\":\"hi\"}");
        assert!(
            pretty.contains("\"level\": \"INFO\""),
            "structured detail is pretty printed:\n{pretty}"
        );
        assert!(pretty.contains('\n'), "pretty JSON spans multiple lines");

        assert_eq!(
            row_detail_text(b"PANIC: unexpected state"),
            "PANIC: unexpected state",
            "raw lines pass through unchanged"
        );

        let long = "x".repeat(DETAIL_MAX_CHARS + 10);
        let truncated = row_detail_text(long.as_bytes());
        assert!(truncated.ends_with("..."));
        assert!(truncated.chars().count() <= DETAIL_MAX_CHARS + 3);
    }

    #[test]
    fn row_selection_clears_on_open_and_reads_detail() {
        let bytes = b"{\"level\": \"INFO\"}\n{\"level\": \"ERROR\", \"message\": \"boom\"}\n";
        let path = temp_file("row_selection.log", bytes);
        let app = &mut LogAnalyzerApp::with_workspace(None);
        // Seed the index directly (background indexing is a separate job).
        let mut file = LoadedFile::open(&path).unwrap();
        file.index = Arc::new(RwLock::new(fixture_index(bytes)));
        app.on_open_result_inner(Ok(file));

        // Selecting a line exposes its pretty-printed JSON.
        app.active_tab_mut().unwrap().selected_line = Some(1);
        assert_eq!(
            row_detail(&app.active_tab().unwrap().file, 1).as_deref(),
            Some("{\n  \"level\": \"ERROR\",\n  \"message\": \"boom\"\n}")
        );
        assert!(app.selected_row_detail().is_some());

        // Opening a file again resets the selection: line numbers may shift.
        app.on_open_result_inner(Ok(LoadedFile::open(&path).unwrap()));
        assert_eq!(app.active_tab().unwrap().selected_line, None);
        assert!(app.selected_row_detail().is_none());
        let _ = std::fs::remove_file(&path);
    }

    // ---- 7.2: scale verification (ignored by default) ----
    //
    // Generate the fixture first:
    //   python3 /tmp/gen_big_log.py /tmp/log_analyzer_synth.log 2
    // Run with:
    //   cargo test --release -- --ignored --nocapture large_file

    #[test]
    #[ignore = "scale verification; generate /tmp/log_analyzer_synth.log first"]
    fn large_file_open_scan_and_cancel_behave_per_spec() {
        let path = std::path::Path::new("/tmp/log_analyzer_synth.log");
        let mut app = LogAnalyzerApp::with_workspace(None);
        let t_open = Instant::now();
        app.open_path(path);
        let open_elapsed = t_open.elapsed();
        let file = &app.active_tab().unwrap().file;

        // Progressive availability: rows appear well before indexing finishes.
        let t_first = Instant::now();
        while file.line_count() == 0 {
            assert!(
                t_first.elapsed() < Duration::from_secs(30),
                "no rows became available while indexing"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let first_rows_elapsed = t_first.elapsed();

        let t_index = Instant::now();
        while !file.index_done() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let index_elapsed = t_index.elapsed();
        let lines = file.line_count();

        let t_fields = Instant::now();
        while app
            .active_tab()
            .unwrap()
            .file
            .fields
            .lock()
            .unwrap()
            .is_empty()
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        let fields_elapsed = t_fields.elapsed();
        app.maybe_apply_columns();
        assert!(!app.active_tab().unwrap().file.visible_columns.is_empty());

        // Query scan on a background thread (as production does).
        let file = &app.active_tab().unwrap().file;
        let q = query::parse("level=ERROR timeout").unwrap();
        let handle = ScanHandle::new();
        let map = Arc::clone(&file.map);
        let index = Arc::clone(&file.index);
        let fields_shared = Arc::clone(&file.fields);
        let worker = handle.clone();
        let t_scan = Instant::now();
        std::thread::spawn(move || {
            let guard = index.read().unwrap();
            run_query_scan(
                &map,
                &guard,
                &q,
                &worker.line_numbers,
                &fields_shared,
                &worker.cancel,
                &worker.scanned,
            );
            worker.done.store(true, Ordering::Relaxed);
        });
        while !handle.done.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let scan_elapsed = t_scan.elapsed();
        let matches = handle.line_numbers.read().unwrap().len();

        // Cancellation: start another scan, cancel mid-flight, require a
        // prompt stop (spec: no freeze; cancellation aborts the scan).
        let handle2 = ScanHandle::new();
        let map = Arc::clone(&file.map);
        let index = Arc::clone(&file.index);
        let fields_shared = Arc::clone(&file.fields);
        let q2 = query::parse("timeout").unwrap();
        let worker = handle2.clone();
        std::thread::spawn(move || {
            let guard = index.read().unwrap();
            run_query_scan(
                &map,
                &guard,
                &q2,
                &worker.line_numbers,
                &fields_shared,
                &worker.cancel,
                &worker.scanned,
            );
            worker.done.store(true, Ordering::Relaxed);
        });
        let target = lines / 10;
        while handle2.scanned.load(Ordering::Relaxed) < target {
            std::thread::sleep(Duration::from_millis(1));
        }
        let t_cancel = Instant::now();
        handle2.cancel.store(true, Ordering::Relaxed);
        while !handle2.done.load(Ordering::Relaxed) {
            assert!(
                t_cancel.elapsed() < Duration::from_secs(5),
                "cancelled scan did not stop promptly"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let cancel_elapsed = t_cancel.elapsed();

        println!("mmap open:            {open_elapsed:?}");
        println!("first rows available: {first_rows_elapsed:?}");
        println!("index build:          {index_elapsed:?} ({lines} lines)");
        println!("first fields found:   {fields_elapsed:?}");
        println!("query scan complete:  {scan_elapsed:?} ({matches} matches)");
        println!("cancel stop latency:  {cancel_elapsed:?}");

        assert!(
            first_rows_elapsed < Duration::from_secs(1),
            "rows must become available progressively, quickly"
        );
        assert!(
            cancel_elapsed < Duration::from_secs(2),
            "cancellation must stop the scan promptly"
        );
    }

    // ---- parse cache ----

    #[test]
    fn parse_cache_evicts_least_recently_used() {
        let mut cache = ParseCache::new(2);
        let a = cache.get_or_compute(1, || {
            Some(serde_json::from_str::<serde_json::Map<String, Value>>(r#"{"k":"1"}"#).unwrap())
        });
        cache.get_or_compute(2, || None);
        // Touch key 1 so key 2 becomes the LRU entry.
        assert!(cache.get_or_compute(1, || None).is_some());
        cache.get_or_compute(3, || None);
        // Key 1 survived; key 2 was evicted.
        assert!(cache.entries.contains_key(&1));
        assert!(!cache.entries.contains_key(&2));
        assert!(a.is_some());
    }
}
