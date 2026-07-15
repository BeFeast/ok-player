use super::*;
use okp_core::history_format::{self, HistoryBucket, HistoryStateKind, LocalDateTime};
use okp_core::recents_shelf::{HistoryItem, WelcomeShelf};
use std::time::{SystemTime, UNIX_EPOCH};

const WELCOME_ITEM_LIMIT: usize = 3;
const OPENED_CONTEXT_REFRESH_SECONDS: i64 = 60;
const HISTORY_LOADING_MILLIS: u64 = 180;
const HISTORY_SEARCH_THRESHOLD: usize = 5;

impl EmptySurface {
    pub(crate) fn refresh(
        &self,
        parent: &gtk::ApplicationWindow,
        state: &Rc<RefCell<PlayerState>>,
        status_toast: Rc<StatusToast>,
    ) {
        let now_unix = unix_now();
        let mut welcome_model = {
            let state = state.borrow();
            state
                .history
                .welcome_shelf(state.private_session, WELCOME_ITEM_LIMIT)
        };
        if env::var("OKP_WELCOME_STATE").ok().as_deref() == Some("empty") {
            welcome_model = WelcomeShelf::Empty;
        }
        let opened_context_bucket = welcome_opened_context_bucket(&welcome_model, now_unix);
        if self.model.borrow().as_ref() != Some(&welcome_model)
            || self.opened_context_bucket.get() != opened_context_bucket
        {
            *self.model.borrow_mut() = Some(welcome_model.clone());
            self.opened_context_bucket.set(opened_context_bucket);
            replace_box_child(
                &self.welcome_host,
                &welcome_page(
                    parent,
                    Rc::clone(state),
                    Rc::clone(&status_toast),
                    &welcome_model,
                    now_unix,
                ),
            );
            self.footer
                .set_visible(!matches!(welcome_model, WelcomeShelf::Empty));
            self.sync_footer(state.borrow().private_session);
        }

        if self.page.get() == IdlePage::History
            && env::var("OKP_HISTORY_STATE").ok().as_deref() != Some("loading")
        {
            let history_model = history_surface_model(state);
            if self.history_model.borrow().as_ref() != Some(&history_model) {
                *self.history_model.borrow_mut() = Some(history_model.clone());
                replace_box_child(
                    &self.history_host,
                    &history_page(
                        self.clone(),
                        parent,
                        Rc::clone(state),
                        Rc::clone(&status_toast),
                        history_model,
                    ),
                );
                self.sync_footer(state.borrow().private_session);
            }
        }
    }

    pub(crate) fn show_history(
        &self,
        parent: &gtk::ApplicationWindow,
        state: Rc<RefCell<PlayerState>>,
        status_toast: Rc<StatusToast>,
    ) {
        if self.page.replace(IdlePage::History) == IdlePage::History {
            return;
        }
        self.footer.set_visible(true);
        self.footer_left_icon
            .set_icon_name(Some("go-previous-symbolic"));
        self.footer_left_label.set_text("Continue watching");
        self.sync_footer(state.borrow().private_session);
        replace_box_child(&self.history_host, &history_loading_page());
        self.stack.set_visible_child_name("history");
        if env::var("OKP_HISTORY_STATE").ok().as_deref() == Some("loading") {
            return;
        }

        let surface = self.clone();
        let parent = parent.clone();
        glib::timeout_add_local_once(Duration::from_millis(HISTORY_LOADING_MILLIS), move || {
            if surface.page.get() != IdlePage::History {
                return;
            }
            let model = history_surface_model(&state);
            *surface.history_model.borrow_mut() = Some(model.clone());
            replace_box_child(
                &surface.history_host,
                &history_page(surface.clone(), &parent, state, status_toast, model),
            );
        });
    }

    pub(crate) fn show_welcome(&self) {
        if self.page.replace(IdlePage::Welcome) == IdlePage::Welcome {
            return;
        }
        self.stack.set_visible_child_name("welcome");
        self.footer_left_icon
            .set_icon_name(Some("document-open-recent-symbolic"));
        self.footer_left_label.set_text("History");
        self.footer.set_visible(!matches!(
            self.model.borrow().as_ref(),
            Some(WelcomeShelf::Empty)
        ));
    }

    fn sync_footer(&self, private_session: bool) {
        self.footer_status.set_text(if private_session {
            "Private mode"
        } else {
            "Recording history"
        });
        if private_session {
            self.footer_status.add_css_class("is-private");
        } else {
            self.footer_status.remove_css_class("is-private");
        }
    }
}

fn welcome_opened_context_bucket(model: &WelcomeShelf, now_unix: i64) -> Option<i64> {
    matches!(model, WelcomeShelf::Items(_))
        .then_some(now_unix.div_euclid(OPENED_CONTEXT_REFRESH_SECONDS))
}

fn welcome_page(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    model: &WelcomeShelf,
    now_unix: i64,
) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-idle-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    let center = gtk::CenterBox::new();
    center.set_hexpand(true);
    center.set_vexpand(true);
    let page = match model {
        WelcomeShelf::Empty => first_run_welcome(parent, state, status_toast),
        WelcomeShelf::Private => private_welcome(parent, state, status_toast),
        WelcomeShelf::Items(items) => {
            continue_watching_welcome(parent, state, status_toast, items, now_unix)
        }
    };
    center.set_center_widget(Some(&page));
    scroller.set_child(Some(&center));
    scroller
}

fn first_run_welcome(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("okp-welcome-first-run");
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);

    content.append(&brand_tile(52, "okp-welcome-brand-tile"));
    let title = gtk::Label::new(Some("Welcome to OK Player"));
    title.add_css_class("okp-first-run-title");
    content.append(&title);

    let copy = gtk::Label::new(Some("Open a file to start playing."));
    copy.add_css_class("okp-first-run-copy");
    content.append(&copy);

    content.append(&welcome_drop_target(parent, state, status_toast, true));
    content
}

fn private_welcome(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("okp-welcome-private");
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);

    let icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
    icon.add_css_class("okp-private-hero-icon");
    icon.set_pixel_size(30);
    content.append(&icon);
    let title = gtk::Label::new(Some("Private session"));
    title.add_css_class("okp-private-hero-title");
    content.append(&title);
    let body = gtk::Label::new(Some(
        "Continue Watching is hidden. New opens will not be recorded or resumed later.",
    ));
    body.add_css_class("okp-private-hero-copy");
    body.set_wrap(true);
    body.set_justify(gtk::Justification::Center);
    body.set_max_width_chars(48);
    content.append(&body);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-private-actions");
    let disable = gtk::Button::with_label("Turn off private session");
    disable.add_css_class("okp-idle-primary-button");
    let private_state = Rc::clone(&state);
    let private_toast = Rc::clone(&status_toast);
    disable.connect_clicked(move |_| toggle_private_session(&private_state, &private_toast));
    actions.append(&disable);
    actions.append(&open_file_button(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    actions.append(&open_url_button(parent, state, status_toast));
    content.append(&actions);
    content
}

fn continue_watching_welcome(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    items: &[HistoryItem],
    now_unix: i64,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("okp-welcome-recents");
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(Some("Continue watching"));
    title.add_css_class("okp-welcome-recents-title");
    title.set_xalign(0.0);
    content.append(&title);
    let subtitle = gtk::Label::new(Some("Pick up where you left off — or open something new."));
    subtitle.add_css_class("okp-welcome-recents-subtitle");
    subtitle.set_xalign(0.0);
    content.append(&subtitle);

    let shelf = gtk::FlowBox::new();
    shelf.add_css_class("okp-recents-shelf");
    shelf.set_selection_mode(gtk::SelectionMode::None);
    shelf.set_homogeneous(true);
    shelf.set_min_children_per_line(1);
    shelf.set_max_children_per_line(3);
    shelf.set_column_spacing(14);
    shelf.set_row_spacing(14);
    shelf.set_halign(gtk::Align::Start);
    for item in items {
        shelf.insert(&recent_card(item, Rc::clone(&state), now_unix), -1);
    }
    content.append(&shelf);

    let actions = gtk::FlowBox::new();
    actions.add_css_class("okp-welcome-action-row");
    actions.set_selection_mode(gtk::SelectionMode::None);
    actions.set_column_spacing(14);
    actions.set_row_spacing(14);
    actions.set_min_children_per_line(1);
    actions.set_max_children_per_line(2);
    let action_column = gtk::Box::new(gtk::Orientation::Vertical, 9);
    action_column.append(&open_file_button(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    action_column.append(&open_url_button(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    actions.insert(&action_column, -1);
    actions.insert(&welcome_drop_target(parent, state, status_toast, false), -1);
    content.append(&actions);
    content
}

fn recent_card(item: &HistoryItem, state: Rc<RefCell<PlayerState>>, now_unix: i64) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-recent-card");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(&item.path));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_size_request(194, -1);
    let thumbnail = history_thumbnail(item, 194, 110, false);
    let progress = gtk::ProgressBar::new();
    progress.add_css_class("okp-recent-progress");
    progress.set_size_request(194, -1);
    progress.set_fraction(item.progress);
    progress.set_valign(gtk::Align::End);
    progress.set_halign(gtk::Align::Fill);
    thumbnail.add_overlay(&progress);
    let remaining = gtk::Label::new(Some(&item.state_label));
    remaining.add_css_class("okp-recent-time-left");
    remaining.set_halign(gtk::Align::End);
    remaining.set_valign(gtk::Align::End);
    remaining.set_margin_end(7);
    remaining.set_margin_bottom(9);
    thumbnail.add_overlay(&remaining);
    content.append(&thumbnail);

    let title = gtk::Label::new(Some(&item.title));
    title.add_css_class("okp-recent-title");
    title.set_xalign(0.0);
    title.set_width_chars(1);
    title.set_ellipsize(pango::EllipsizeMode::End);
    content.append(&title);
    let location = gtk::Label::new(Some(&item.location));
    location.add_css_class("okp-recent-location");
    location.set_xalign(0.0);
    location.set_ellipsize(pango::EllipsizeMode::Middle);
    content.append(&location);
    let context = gtk::Label::new(Some(&format!(
        "{} · {}",
        okp_core::recents_shelf::runtime_label(item.duration),
        okp_core::recents_shelf::opened_context(item.updated_at_unix, now_unix)
    )));
    context.add_css_class("okp-recent-context");
    context.set_xalign(0.0);
    content.append(&context);
    button.set_child(Some(&content));

    let path = PathBuf::from(&item.path);
    button.connect_clicked(move |_| load_media_path(&state, path.clone()));
    button
}

fn open_file_button(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Button {
    let button = icon_text_button("document-open-symbolic", "Open file…");
    button.add_css_class("okp-idle-primary-button");
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        open_media_dialog(&parent, Rc::clone(&state), Rc::clone(&status_toast))
    });
    button
}

fn open_url_button(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Button {
    let button = icon_text_button("insert-link-symbolic", "Open URL…");
    button.add_css_class("okp-idle-secondary-button");
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        open_url_dialog(&parent, Rc::clone(&state), Rc::clone(&status_toast));
    });
    button
}

fn welcome_drop_target(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    first_run: bool,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(if first_run {
        "okp-first-run-drop-target"
    } else {
        "okp-welcome-drop-target"
    });
    button.set_has_frame(false);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);
    if !first_run {
        let icon = gtk::Image::from_icon_name("document-send-symbolic");
        icon.set_pixel_size(22);
        content.append(&icon);
    }
    let primary = gtk::Label::new(Some(if first_run {
        "Open file…"
    } else {
        "Drop a video, folder, or link"
    }));
    primary.add_css_class("okp-drop-primary");
    content.append(&primary);
    if first_run {
        let secondary = gtk::Label::new(Some("or drop a file, folder, or link"));
        secondary.add_css_class("okp-drop-secondary");
        content.append(&secondary);
    }
    button.set_child(Some(&content));
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        open_media_dialog(&parent, Rc::clone(&state), Rc::clone(&status_toast))
    });
    button
}

pub(crate) fn brand_tile(size: i32, css_class: &str) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    tile.add_css_class("okp-brand-tile");
    tile.add_css_class(css_class);
    tile.set_size_request(size, size);
    tile.set_halign(gtk::Align::Center);
    tile.set_valign(gtk::Align::Center);
    let ok = gtk::Label::new(Some("OK"));
    ok.add_css_class("okp-brand-ok");
    let play = gtk::Label::new(Some("▶"));
    play.add_css_class("okp-brand-play");
    tile.append(&ok);
    tile.append(&play);
    tile
}

pub(crate) fn idle_titlebar() -> gtk::Box {
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    titlebar.add_css_class("okp-idle-titlebar");
    let mark = gtk::Label::new(Some("▶"));
    mark.add_css_class("okp-idle-titlebar-mark");
    titlebar.append(&mark);
    let title = gtk::Label::new(Some("OK Player"));
    title.add_css_class("okp-idle-titlebar-text");
    titlebar.append(&title);
    titlebar
}

pub(crate) fn idle_footer_widgets() -> (gtk::Box, gtk::Button, gtk::Image, gtk::Label, gtk::Label) {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("okp-idle-footer");
    let left = gtk::Button::new();
    left.add_css_class("okp-idle-footer-button");
    let left_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let icon = gtk::Image::from_icon_name("document-open-recent-symbolic");
    icon.set_pixel_size(13);
    let label = gtk::Label::new(Some("History"));
    left_content.append(&icon);
    left_content.append(&label);
    left.set_child(Some(&left_content));
    footer.append(&left);

    let status = gtk::Label::new(Some("Recording history"));
    status.add_css_class("okp-idle-footer-status");
    status.set_hexpand(true);
    status.set_halign(gtk::Align::Center);
    footer.append(&status);
    (footer, left, icon, label, status)
}

pub(crate) fn idle_footer_settings_button(
    footer: &gtk::Box,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let settings = gtk::Button::from_icon_name("emblem-system-symbolic");
    settings.add_css_class("okp-idle-footer-button");
    settings.set_tooltip_text(Some("Settings"));
    let parent = parent.clone();
    settings.connect_clicked(move |_| {
        open_settings_window(&parent, Rc::clone(&state), Rc::clone(&status_toast));
    });
    footer.append(&settings);
}

fn history_surface_model(state: &Rc<RefCell<PlayerState>>) -> HistorySurfaceModel {
    let state = state.borrow();
    let preview = env::var("OKP_HISTORY_STATE").ok();
    let mut items = state.history.search("");
    let read_failed = preview.as_deref() == Some("error") || state.history.read_failed();
    let cleared = preview.as_deref() == Some("cleared") || state.history.was_cleared();
    let no_match = preview.as_deref() == Some("no-match");
    if matches!(
        preview.as_deref(),
        Some("empty") | Some("cleared") | Some("error")
    ) {
        items.clear();
    }
    HistorySurfaceModel {
        items,
        private_session: state.private_session,
        read_failed,
        cleared,
        no_match,
    }
}

fn history_loading_page() -> gtk::ScrolledWindow {
    let (page, content) = history_page_shell();
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rows.add_css_class("okp-history-loading");
    let caption = gtk::Box::new(gtk::Orientation::Vertical, 0);
    caption.add_css_class("okp-history-skeleton-caption");
    rows.append(&caption);
    for _ in 0..7 {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 13);
        row.add_css_class("okp-history-skeleton-row");
        let thumb = gtk::Box::new(gtk::Orientation::Vertical, 0);
        thumb.add_css_class("okp-history-skeleton-thumb");
        row.append(&thumb);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 7);
        text.set_hexpand(true);
        let line1 = gtk::Box::new(gtk::Orientation::Vertical, 0);
        line1.add_css_class("okp-history-skeleton-line-1");
        let line2 = gtk::Box::new(gtk::Orientation::Vertical, 0);
        line2.add_css_class("okp-history-skeleton-line-2");
        text.append(&line1);
        text.append(&line2);
        row.append(&text);
        rows.append(&row);
    }
    content.append(&rows);
    page
}

fn history_page(
    surface: EmptySurface,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    model: HistorySurfaceModel,
) -> gtk::ScrolledWindow {
    let (page, content) = history_page_shell();

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 11);
    header.add_css_class("okp-history-header");
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("okp-history-back-button");
    back.connect_clicked(move |_| surface.show_welcome());
    header.append(&back);
    let title = gtk::Label::new(Some("History"));
    title.add_css_class("okp-history-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    content.append(&header);

    let subtitle = gtk::Label::new(Some("Everything you’ve opened · keeping last 90 days"));
    subtitle.add_css_class("okp-history-subtitle");
    subtitle.set_xalign(0.0);
    content.append(&subtitle);
    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-history-divider");
    content.append(&divider);

    if model.private_session && !model.read_failed && !model.items.is_empty() {
        let banner = gtk::Label::new(Some(
            "Private mode — new opens aren’t being recorded. Your existing history is still here.",
        ));
        banner.add_css_class("okp-history-private-banner");
        banner.set_wrap(true);
        banner.set_xalign(0.0);
        content.append(&banner);
    }

    if model.read_failed {
        content.append(&history_error_state(state, status_toast));
        return page;
    }
    if model.items.is_empty() {
        content.append(&history_empty_state(model.cleared));
        return page;
    }
    if model.no_match {
        content.append(&history_no_matches_state());
        return page;
    }

    let rows_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let all_items = Rc::new(model.items);
    let render_rows = {
        let rows_host = rows_host.clone();
        let all_items = Rc::clone(&all_items);
        let state = Rc::clone(&state);
        let parent = parent.clone();
        let status_toast = Rc::clone(&status_toast);
        move |query: &str| {
            render_history_rows(
                &rows_host,
                &all_items,
                query,
                Rc::clone(&state),
                &parent,
                Rc::clone(&status_toast),
            );
        }
    };

    if all_items.len() > HISTORY_SEARCH_THRESHOLD {
        let search = gtk::SearchEntry::new();
        search.add_css_class("okp-history-search");
        search.set_placeholder_text(Some("Search…"));
        search.set_width_chars(24);
        header.append(&search);
        let render = Rc::new(render_rows);
        let render_changed = Rc::clone(&render);
        search.connect_search_changed(move |entry| render_changed(entry.text().as_str()));
        render("");
    } else {
        render_rows("");
    }
    content.append(&rows_host);
    page
}

fn history_page_shell() -> (gtk::ScrolledWindow, gtk::Box) {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-history-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("okp-history-page");
    content.set_halign(gtk::Align::Center);
    scroller.set_child(Some(&content));
    (scroller, content)
}

fn render_history_rows(
    host: &gtk::Box,
    items: &[HistoryItem],
    query: &str,
    state: Rc<RefCell<PlayerState>>,
    parent: &gtk::ApplicationWindow,
    status_toast: Rc<StatusToast>,
) {
    clear_box(host);
    let query = query.trim().to_lowercase();
    let filtered = items
        .iter()
        .filter(|item| {
            query.is_empty()
                || item.title.to_lowercase().contains(&query)
                || item.location.to_lowercase().contains(&query)
        })
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        host.append(&history_no_matches_state());
        return;
    }
    if !query.is_empty() {
        let caption = gtk::Label::new(Some(&format!(
            "{} result{}",
            filtered.len(),
            if filtered.len() == 1 { "" } else { "s" }
        )));
        caption.add_css_class("okp-history-result-caption");
        caption.set_xalign(0.0);
        host.append(&caption);
        for item in filtered {
            host.append(&history_row(
                item,
                Rc::clone(&state),
                parent,
                Rc::clone(&status_toast),
            ));
        }
        return;
    }

    let now = local_datetime(unix_now()).unwrap_or_else(fallback_local_datetime);
    for bucket in [
        HistoryBucket::Today,
        HistoryBucket::Yesterday,
        HistoryBucket::EarlierThisWeek,
        HistoryBucket::Earlier,
    ] {
        let bucket_items = filtered
            .iter()
            .copied()
            .filter(|item| {
                local_datetime(item.updated_at_unix)
                    .is_some_and(|when| history_format::bucket_for(when, now) == bucket)
            })
            .collect::<Vec<_>>();
        if bucket_items.is_empty() {
            continue;
        }
        let header = gtk::Label::new(Some(history_format::bucket_header(bucket)));
        header.add_css_class("okp-history-bucket");
        header.set_xalign(0.0);
        host.append(&header);
        for item in bucket_items {
            host.append(&history_row(
                item,
                Rc::clone(&state),
                parent,
                Rc::clone(&status_toast),
            ));
        }
    }
    let end = gtk::Label::new(Some("End of history · keeping last 90 days"));
    end.add_css_class("okp-history-end-cap");
    host.append(&end);
}

fn history_row(
    item: &HistoryItem,
    state: Rc<RefCell<PlayerState>>,
    parent: &gtk::ApplicationWindow,
    status_toast: Rc<StatusToast>,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-history-row");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button.set_tooltip_text(Some(&item.path));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 13);
    row.append(&history_thumbnail(
        item,
        64,
        36,
        item.state_kind == HistoryStateKind::Finished,
    ));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(&item.title));
    title.add_css_class("okp-history-row-title");
    title.set_xalign(0.0);
    title.set_width_chars(1);
    title.set_ellipsize(pango::EllipsizeMode::End);
    text.append(&title);
    let location = gtk::Label::new(Some(&item.location));
    location.add_css_class("okp-history-row-location");
    location.set_xalign(0.0);
    location.set_ellipsize(pango::EllipsizeMode::End);
    text.append(&location);
    row.append(&text);

    let right = gtk::Box::new(gtk::Orientation::Vertical, 5);
    right.set_halign(gtk::Align::End);
    let when = local_datetime(item.updated_at_unix)
        .map(|value| history_format::when_label(value, fallback_local_datetime()))
        .unwrap_or_else(|| "Opened previously".to_owned());
    let when = gtk::Label::new(Some(&when));
    when.add_css_class("okp-history-row-when");
    when.set_halign(gtk::Align::End);
    right.append(&when);
    match item.state_kind {
        HistoryStateKind::Finished => {
            let state_label = gtk::Label::new(Some("✓ Finished"));
            state_label.add_css_class("okp-history-finished-chip");
            state_label.set_halign(gtk::Align::End);
            right.append(&state_label);
        }
        HistoryStateKind::Progress => {
            let state_label = gtk::Label::new(Some(&item.state_label));
            state_label.add_css_class("okp-history-progress-label");
            state_label.set_halign(gtk::Align::End);
            right.append(&state_label);
        }
        HistoryStateKind::Barely => {
            let state_label = gtk::Label::new(Some(&item.state_label));
            state_label.add_css_class("okp-history-barely-label");
            state_label.set_halign(gtk::Align::End);
            right.append(&state_label);
        }
    }
    row.append(&right);
    button.set_child(Some(&row));

    let path = PathBuf::from(&item.path);
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        if !path.is_file() {
            status_toast.show("History file is no longer available");
            return;
        }
        load_media_path(&state, path.clone());
        parent.present();
    });
    button
}

fn history_thumbnail(item: &HistoryItem, width: i32, height: i32, finished: bool) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-history-thumbnail");
    if finished {
        overlay.add_css_class("is-finished");
    }
    overlay.set_size_request(width, height);
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("okp-history-thumbnail-placeholder");
    frame.set_size_request(width, height);
    let placeholder = gtk::Image::from_icon_name("video-x-generic-symbolic");
    placeholder.add_css_class("okp-history-thumbnail-icon");
    placeholder.set_pixel_size(if width <= 64 { 16 } else { 24 });
    placeholder.set_halign(gtk::Align::Center);
    placeholder.set_valign(gtk::Align::Center);
    frame.append(&placeholder);
    overlay.set_child(Some(&frame));
    if let Some(path) = item
        .poster_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_file())
    {
        let picture = gtk::Picture::for_filename(path);
        picture.add_css_class("okp-history-thumbnail-picture");
        picture.set_size_request(width, height);
        picture.set_can_shrink(true);
        overlay.add_overlay(&picture);
    }
    if item.state_kind == HistoryStateKind::Progress {
        let progress = gtk::ProgressBar::new();
        progress.add_css_class("okp-history-thumb-progress");
        progress.set_size_request(width, -1);
        progress.set_fraction(item.progress);
        progress.set_valign(gtk::Align::End);
        overlay.add_overlay(&progress);
    }
    overlay
}

fn history_error_state(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let card = history_state_card(
        "dialog-warning-symbolic",
        "Couldn’t read your history just now",
        "The history file may be temporarily unavailable or damaged.",
    );
    let retry = gtk::Button::with_label("Retry");
    retry.add_css_class("okp-history-state-button");
    retry.set_halign(gtk::Align::Center);
    retry.connect_clicked(move |_| {
        state.borrow_mut().history.retry_read();
        status_toast.show("History reloaded");
    });
    card.append(&retry);
    card
}

fn history_empty_state(cleared: bool) -> gtk::Box {
    history_state_card(
        "document-open-recent-symbolic",
        if cleared {
            "History cleared"
        } else {
            "Nothing here yet"
        },
        if cleared {
            "Nothing left to show. New files you open will start a fresh history."
        } else {
            "Files you open show up in History."
        },
    )
}

fn history_no_matches_state() -> gtk::Box {
    history_state_card(
        "edit-find-symbolic",
        "No matches",
        "Nothing in your history matches this search.",
    )
}

fn history_state_card(icon_name: &str, title: &str, body: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 11);
    card.add_css_class("okp-history-state-card");
    card.set_halign(gtk::Align::Center);
    let icon_wrap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    icon_wrap.add_css_class("okp-history-state-icon-wrap");
    icon_wrap.set_size_request(54, 54);
    icon_wrap.set_halign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(22);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon_wrap.append(&icon);
    card.append(&icon_wrap);
    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-history-state-title");
    card.append(&title);
    let body = gtk::Label::new(Some(body));
    body.add_css_class("okp-history-state-body");
    body.set_wrap(true);
    body.set_justify(gtk::Justification::Center);
    body.set_max_width_chars(42);
    card.append(&body);
    card
}

fn icon_text_button(icon_name: &str, text: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_has_frame(false);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(15);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(text)));
    button.set_child(Some(&content));
    button
}

fn replace_box_child(container: &gtk::Box, child: &impl IsA<gtk::Widget>) {
    clear_box(container);
    container.append(child);
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn local_datetime(unix: i64) -> Option<LocalDateTime> {
    let datetime = glib::DateTime::from_unix_local(unix).ok()?;
    Some(LocalDateTime::new(
        datetime.year(),
        datetime.month() as u32,
        datetime.day_of_month() as u32,
        datetime.hour() as u32,
        datetime.minute() as u32,
    ))
}

fn fallback_local_datetime() -> LocalDateTime {
    local_datetime(unix_now()).unwrap_or_else(|| LocalDateTime::new(1970, 1, 1, 0, 0))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_items_refresh_relative_context_once_per_minute() {
        let items = WelcomeShelf::Items(Vec::new());
        assert_eq!(welcome_opened_context_bucket(&items, 119), Some(1));
        assert_eq!(welcome_opened_context_bucket(&items, 120), Some(2));
        assert_eq!(
            welcome_opened_context_bucket(&WelcomeShelf::Empty, 120),
            None
        );
        assert_eq!(
            welcome_opened_context_bucket(&WelcomeShelf::Private, 120),
            None
        );
    }
}
