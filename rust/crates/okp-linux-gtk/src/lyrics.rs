use super::*;

/// The audio lyrics surface: an Apple-Music-style overlay that fills the (video-less) plane while an
/// audio file plays. It discovers a sidecar `.lrc` through [`okp_core::lyrics`], parses it with the
/// shared [`okp_core::lrc`] core, and renders the whole sheet — the active line brightened, the rest
/// dimmed. Synced sheets scroll the active line toward centre and each line seeks on click; a plain
/// (untimed) sheet reads as a static scroll; a missing sheet shows a calm empty state.
///
/// It is audio-only by design: [`current_audio_path`] gates on [`media_formats::is_audio`], so a
/// video file never reveals it and the video-first player stays untouched. The parsing and the
/// active-line selection live in the core; this module is UI and orchestration only.
pub(crate) struct LyricsSurface {
    revealer: gtk::Revealer,
    header: gtk::Label,
    scroller: gtk::ScrolledWindow,
    list: gtk::Box,
    state: LyricsRenderState,
}

/// Cached render state so the 200 ms poll rebuilds the row widgets only when the track changes and
/// otherwise just moves the highlight. `rows` is aligned 1:1 with `lines` for a synced sheet (the
/// widget each `is-active` toggle lands on); it is empty for plain lyrics and the empty state, where
/// there is no per-line highlight to drive.
#[derive(Default)]
struct LyricsRenderState {
    loaded_key: RefCell<Option<String>>,
    lines: RefCell<Vec<lrc::LrcLine>>,
    rows: RefCell<Vec<gtk::Widget>>,
    has_timings: Cell<bool>,
    active: Cell<Option<usize>>,
    // The visual smoke hook (`OKP_OPEN_LYRICS_ON_STARTUP`) freezes the poll on fixture lyrics; the
    // freeze releases the moment real audio media loads, so an inherited env var can never pin the
    // fixture over a real session.
    preview_frozen: Cell<bool>,
}

const LYRICS_PREVIEW_KEY: &str = "__okp_lyrics_preview__";

/// Which fixture the visual smoke hook renders (`OKP_OPEN_LYRICS_ON_STARTUP`): a synced sheet with a
/// live highlight, a plain (untimed) sheet, or the empty state.
#[derive(Clone, Copy)]
pub(crate) enum LyricsPreviewMode {
    Synced,
    Plain,
    Empty,
}

const LYRICS_PREVIEW_PLAIN: &str = "Wander down the boulevard\n\
    Counting every falling star\n\
    Nothing here but you and me\n\
    And the hum of the city, endlessly";

pub(crate) fn build_lyrics_surface() -> LyricsSurface {
    let header = gtk::Label::new(Some("LYRICS"));
    header.add_css_class("okp-lyrics-header");
    header.set_halign(gtk::Align::Center);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("okp-lyrics-list");
    list.set_halign(gtk::Align::Center);
    list.set_valign(gtk::Align::Start);

    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-lyrics-scroller");
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_child(Some(&list));

    // The scrim background lives on the revealer itself (the proven empty-surface pattern) so it
    // fills the whole plane; the content column is centred and capped inside it.
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("okp-lyrics-content");
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Fill);
    content.append(&header);
    content.append(&scroller);

    let revealer = gtk::Revealer::new();
    revealer.add_css_class("okp-lyrics-surface");
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Fill);
    revealer.set_transition_duration(180);
    revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    revealer.set_reveal_child(false);
    revealer.set_can_target(false);
    // The scrim background lives on the revealer, so a collapsed child alone is not
    // enough: the full-size overlay would still paint over video and the welcome canvas.
    revealer.set_visible(false);
    revealer.set_child(Some(&content));

    LyricsSurface {
        revealer,
        header,
        scroller,
        list,
        state: LyricsRenderState::default(),
    }
}

impl LyricsSurface {
    pub(crate) fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    /// True while the visual smoke hook is pinning a fixture sheet. The poll uses this to keep the
    /// welcome surface hidden underneath the preview (in production the welcome surface is already
    /// hidden by loaded media, so this is always false there).
    pub(crate) fn is_preview_frozen(&self) -> bool {
        self.state.preview_frozen.get()
    }

    /// Reconcile the surface against the current media: hide it for video / no media, reveal it for
    /// audio, (re)build the sheet on a track change, and move the highlight to the current position.
    pub(crate) fn update(&self, state: &Rc<RefCell<PlayerState>>) {
        if self.state.preview_frozen.get() {
            if current_audio_path(state).is_some() {
                self.state.preview_frozen.set(false);
            } else {
                return;
            }
        }

        let Some(path) = current_audio_path(state) else {
            self.hide();
            return;
        };

        self.reveal();
        let key = path.to_string_lossy().into_owned();
        if self.state.loaded_key.borrow().as_deref() != Some(key.as_str()) {
            let document = okp_core::lyrics::read_sidecar(&path)
                .map(|text| lrc::parse(Some(&text)))
                .unwrap_or_default();
            self.rebuild(&document, state);
            self.state.loaded_key.replace(Some(key));
        }

        self.update_highlight(observed_position(state));
    }

    fn reveal(&self) {
        self.revealer.set_visible(true);
        self.revealer.set_reveal_child(true);
        self.revealer.set_can_target(true);
    }

    fn hide(&self) {
        self.revealer.set_reveal_child(false);
        self.revealer.set_can_target(false);
        self.revealer.set_visible(false);
        if self.state.loaded_key.borrow().is_none() {
            return;
        }
        self.state.loaded_key.replace(None);
        self.state.active.set(None);
    }

    /// Replace the rendered sheet from a freshly parsed document. Clears the old rows, resets the
    /// highlight, and picks the header/body for the synced / plain / empty case.
    fn rebuild(&self, document: &lrc::LrcDocument, state: &Rc<RefCell<PlayerState>>) {
        clear_list_box_children(&self.list);
        self.state.rows.borrow_mut().clear();
        self.state.lines.borrow_mut().clear();
        self.state.active.set(None);
        self.state.has_timings.set(document.has_timings);

        if document.is_empty() {
            self.header.set_text("LYRICS");
            self.list.set_valign(gtk::Align::Center);
            self.list.append(&lyrics_empty_row());
            return;
        }

        self.list.set_valign(gtk::Align::Start);
        if document.has_timings {
            self.header.set_text("LYRICS");
            let mut rows = Vec::with_capacity(document.lines.len());
            for line in &document.lines {
                let row = synced_lyric_row(line, state);
                self.list.append(&row);
                rows.push(row);
            }
            self.state.rows.replace(rows);
            self.state.lines.replace(document.lines.clone());
        } else {
            self.header.set_text("LYRICS · NOT SYNCED");
            for line in &document.lines {
                self.list.append(&plain_lyric_row(&line.text));
            }
        }
    }

    /// Move the active-line highlight to the line at `position` (synced sheets only). No-op for
    /// plain lyrics and the empty state, which carry no per-line highlight.
    fn update_highlight(&self, position: f64) {
        if !self.state.has_timings.get() {
            return;
        }
        let index = lrc::active_index(&self.state.lines.borrow(), position);
        if index == self.state.active.get() {
            return;
        }

        let rows = self.state.rows.borrow();
        if let Some(previous) = self.state.active.get().and_then(|i| rows.get(i)) {
            previous.remove_css_class("is-active");
        }
        self.state.active.set(index);
        if let Some(active) = index.and_then(|i| rows.get(i)) {
            active.add_css_class("is-active");
            scroll_row_into_view(&self.scroller, &self.list, active);
        }
    }

    /// Visual smoke hook: freeze the poll and render a fixture sheet so the surface can be
    /// screenshot-tested without loaded media. Presentational only; production never calls this.
    pub(crate) fn open_preview(&self, state: &Rc<RefCell<PlayerState>>, mode: LyricsPreviewMode) {
        self.state.preview_frozen.set(true);
        let (document, position) = match mode {
            LyricsPreviewMode::Synced => lyrics_preview_sample(),
            LyricsPreviewMode::Plain => (lrc::parse(Some(LYRICS_PREVIEW_PLAIN)), 0.0),
            LyricsPreviewMode::Empty => (lrc::LrcDocument::default(), 0.0),
        };
        self.reveal();
        self.rebuild(&document, state);
        self.state
            .loaded_key
            .replace(Some(LYRICS_PREVIEW_KEY.to_owned()));
        self.update_highlight(position);
    }
}

/// The local audio path whose lyrics should show, or `None` for video / a stream / no media. The
/// extension gate keeps the surface off the video-first player entirely, mirroring the Windows
/// `MediaFormats.IsAudio` half of its audio-surface gate.
pub(crate) fn current_audio_path(state: &Rc<RefCell<PlayerState>>) -> Option<PathBuf> {
    let state = state.borrow();
    let path = state.current_file.as_ref()?;
    media_formats::is_audio(path).then(|| path.clone())
}

fn observed_position(state: &Rc<RefCell<PlayerState>>) -> f64 {
    state
        .borrow()
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.observed_playback_state().time_pos)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// One synced line: a flat button so a click seeks to its timestamp (Apple-Music-style tap-to-jump).
/// An empty line is a carried instrumental gap — render a quiet rest glyph rather than a blank,
/// clickable-looking row.
fn synced_lyric_row(line: &lrc::LrcLine, state: &Rc<RefCell<PlayerState>>) -> gtk::Widget {
    let is_gap = line.text.trim().is_empty();
    let label = gtk::Label::new(Some(if is_gap { "♪" } else { line.text.as_str() }));
    label.set_wrap(true);
    label.set_justify(gtk::Justification::Center);
    label.set_max_width_chars(48);

    let button = gtk::Button::new();
    button.add_css_class("okp-lyrics-line");
    if is_gap {
        button.add_css_class("is-gap");
    }
    button.set_has_frame(false);
    button.set_child(Some(&label));

    let time = line.time_seconds;
    let seek_state = Rc::clone(state);
    button.connect_clicked(move |_| {
        seek_to_time(&seek_state, time);
    });
    button.upcast()
}

fn plain_lyric_row(text: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-lyrics-line-plain");
    label.set_wrap(true);
    label.set_justify(gtk::Justification::Center);
    label.set_max_width_chars(48);
    label.set_halign(gtk::Align::Center);
    label.upcast()
}

fn lyrics_empty_row() -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-lyrics-empty");
    row.set_halign(gtk::Align::Center);
    row.set_valign(gtk::Align::Center);

    let icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
    icon.add_css_class("okp-lyrics-empty-icon");
    icon.set_pixel_size(30);
    row.append(&icon);

    let label = gtk::Label::new(Some("No lyrics found for this track"));
    label.add_css_class("okp-lyrics-empty-text");
    label.set_justify(gtk::Justification::Center);
    label.set_wrap(true);
    label.set_max_width_chars(30);
    row.append(&label);

    row.upcast()
}

fn clear_list_box_children(list: &gtk::Box) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

/// Scroll the active row toward the vertical centre of the viewport. Defensive: if the row has no
/// resolved bounds yet (a poll tick before the first layout), the highlight still lands and the
/// scroll settles on the next tick.
fn scroll_row_into_view(scroller: &gtk::ScrolledWindow, list: &gtk::Box, row: &gtk::Widget) {
    let Some(bounds) = row.compute_bounds(list) else {
        return;
    };
    let adjustment = scroller.vadjustment();
    let row_center = f64::from(bounds.y()) + f64::from(bounds.height()) / 2.0;
    let target = row_center - adjustment.page_size() / 2.0;
    let ceiling = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    adjustment.set_value(target.clamp(adjustment.lower(), ceiling));
}

/// Representative synced lyrics for the visual smoke hook. Built through the real core parser so the
/// fixture exercises the same path production does. The position lands on a mid-sheet line so the
/// active-line brightening is captured. Fixture only — the live surface always renders from a
/// discovered sidecar.
pub(crate) fn lyrics_preview_sample() -> (lrc::LrcDocument, f64) {
    let sheet = "[ti:Neon Skyline]\n\
        [ar:The Wander Club]\n\
        [00:00.00]City lights blur into the rain\n\
        [00:05.40]Every window holds a name\n\
        [00:11.20]We were chasing something bright\n\
        [00:16.80]A neon skyline out of sight\n\
        [00:22.50]So hold the night a little longer\n\
        [00:28.30]Every heartbeat pulling stronger\n\
        [00:34.10]Down the avenue we go\n\
        [00:39.90]Where the quiet rivers flow\n";
    (lrc::parse(Some(sheet)), 17.5)
}
