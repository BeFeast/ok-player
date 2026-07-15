use super::*;
use okp_core::history_format::HistoryStateKind;
use okp_core::recents_shelf::{self, HistoryItem, WelcomeShelf};
use std::time::{SystemTime, UNIX_EPOCH};

const WELCOME_ITEM_LIMIT: usize = 3;
const OPENED_CONTEXT_REFRESH_SECONDS: i64 = 60;

impl EmptySurface {
    /// Refresh the idle canvas when history, private-session state, or the displayed minute
    /// changes. The equality guard avoids rebuilding GTK widgets on every 200 ms player poll.
    pub(crate) fn refresh(
        &self,
        parent: &gtk::ApplicationWindow,
        state: &Rc<RefCell<PlayerState>>,
        status_toast: Rc<StatusToast>,
    ) {
        let now_unix = unix_now();
        let model = {
            let state = state.borrow();
            state
                .history
                .welcome_shelf(state.private_session, WELCOME_ITEM_LIMIT)
        };
        let opened_context_bucket = welcome_opened_context_bucket(&model, now_unix);
        if self.model.borrow().as_ref() == Some(&model)
            && self.opened_context_bucket.get() == opened_context_bucket
        {
            return;
        }

        *self.model.borrow_mut() = Some(model.clone());
        self.opened_context_bucket.set(opened_context_bucket);
        clear_box(&self.content);
        match model {
            WelcomeShelf::Private => self.content.append(&private_welcome(
                parent,
                Rc::clone(state),
                Rc::clone(&status_toast),
            )),
            WelcomeShelf::Empty => self.content.append(&first_run_welcome(
                parent,
                Rc::clone(state),
                Rc::clone(&status_toast),
            )),
            WelcomeShelf::Items(items) => self.content.append(&continue_watching_welcome(
                parent,
                Rc::clone(state),
                Rc::clone(&status_toast),
                &items,
                now_unix,
            )),
        }
    }
}

fn welcome_opened_context_bucket(model: &WelcomeShelf, now_unix: i64) -> Option<i64> {
    matches!(model, WelcomeShelf::Items(_))
        .then_some(now_unix.div_euclid(OPENED_CONTEXT_REFRESH_SECONDS))
}

fn first_run_welcome(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("okp-welcome-first-run");
    content.set_halign(gtk::Align::Center);

    content.append(&empty_surface_logo());
    content.append(&welcome_wordmark());

    let tagline = gtk::Label::new(Some("Open a file to start playing."));
    tagline.add_css_class("okp-empty-tagline");
    tagline.set_justify(gtk::Justification::Center);
    tagline.set_wrap(true);
    tagline.set_max_width_chars(34);
    content.append(&tagline);
    content.append(&welcome_actions(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        true,
    ));
    content.append(&welcome_hint());
    content.append(&welcome_footer(parent, state, status_toast, false));
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

    let icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
    icon.add_css_class("okp-welcome-private-icon");
    icon.set_pixel_size(38);
    content.append(&icon);

    let title = gtk::Label::new(Some("Private session"));
    title.add_css_class("okp-welcome-private-title");
    content.append(&title);

    let body = gtk::Label::new(Some(
        "Continue Watching is hidden. Files opened now will not be added to history or resumed later.",
    ));
    body.add_css_class("okp-welcome-private-body");
    body.set_wrap(true);
    body.set_justify(gtk::Justification::Center);
    body.set_max_width_chars(48);
    content.append(&body);

    let disable = gtk::Button::with_label("Turn off private session");
    disable.add_css_class("okp-empty-primary-button");
    let private_state = Rc::clone(&state);
    let private_toast = Rc::clone(&status_toast);
    disable.connect_clicked(move |_| toggle_private_session(&private_state, &private_toast));
    content.append(&disable);

    content.append(&welcome_actions(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        false,
    ));
    content.append(&welcome_footer(parent, state, status_toast, true));
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

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    heading.add_css_class("okp-welcome-recents-heading");
    heading.append(&app_identity_image(44, "okp-recents-mark"));

    let heading_copy = gtk::Box::new(gtk::Orientation::Vertical, 0);
    heading_copy.set_hexpand(true);
    let title = gtk::Label::new(Some("Continue Watching"));
    title.add_css_class("okp-welcome-recents-title");
    title.set_xalign(0.0);
    heading_copy.append(&title);

    let subtitle = gtk::Label::new(Some("Pick up where you left off, or open something new."));
    subtitle.add_css_class("okp-welcome-recents-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    heading_copy.append(&subtitle);
    heading.append(&heading_copy);
    content.append(&heading);

    content.append(&welcome_actions(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        false,
    ));

    let shelf = gtk::FlowBox::new();
    shelf.add_css_class("okp-recents-shelf");
    shelf.set_selection_mode(gtk::SelectionMode::None);
    shelf.set_homogeneous(true);
    shelf.set_min_children_per_line(1);
    shelf.set_max_children_per_line(3);
    shelf.set_column_spacing(14);
    shelf.set_row_spacing(14);
    shelf.set_halign(gtk::Align::Fill);
    for item in items {
        shelf.insert(&recent_card(item, Rc::clone(&state), now_unix), -1);
    }
    content.append(&shelf);

    content.append(&welcome_hint());
    content.append(&welcome_footer(parent, state, status_toast, false));
    content
}

fn recent_card(item: &HistoryItem, state: Rc<RefCell<PlayerState>>, now_unix: i64) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-recent-card");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(&item.path));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_size_request(194, -1);

    let thumbnail = history_thumbnail(item, 194, 108);
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
    remaining.set_margin_end(8);
    remaining.set_margin_bottom(9);
    thumbnail.add_overlay(&remaining);
    content.append(&thumbnail);

    let title = gtk::Label::new(Some(&item.title));
    title.add_css_class("okp-recent-title");
    title.set_xalign(0.0);
    title.set_width_chars(1);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);
    content.append(&title);

    let location = gtk::Label::new(Some(&item.location));
    location.add_css_class("okp-recent-location");
    location.set_xalign(0.0);
    location.set_width_chars(1);
    location.set_hexpand(true);
    location.set_ellipsize(pango::EllipsizeMode::Middle);
    content.append(&location);

    let context = gtk::Label::new(Some(&format!(
        "{} · {}",
        recents_shelf::runtime_label(item.duration),
        recents_shelf::opened_context(item.updated_at_unix, now_unix)
    )));
    context.add_css_class("okp-recent-context");
    context.set_xalign(0.0);
    content.append(&context);

    button.set_child(Some(&content));
    let path = PathBuf::from(&item.path);
    button.connect_clicked(move |_| {
        load_media_path(&state, path.clone());
    });
    button
}

fn history_thumbnail(item: &HistoryItem, width: i32, height: i32) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-history-thumbnail");
    overlay.set_size_request(width, height);
    overlay.set_hexpand(false);
    overlay.set_vexpand(false);

    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("okp-history-thumbnail-placeholder");
    frame.set_size_request(width, height);
    frame.set_hexpand(false);
    frame.set_vexpand(false);
    let placeholder = gtk::Image::from_icon_name("video-x-generic-symbolic");
    placeholder.add_css_class("okp-history-thumbnail-icon");
    placeholder.set_pixel_size(24);
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
    overlay
}

fn welcome_wordmark() -> gtk::Box {
    let wordmark = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    wordmark.add_css_class("okp-empty-wordmark");
    wordmark.set_halign(gtk::Align::Center);
    let wordmark_ok = gtk::Label::new(Some("OK"));
    wordmark_ok.add_css_class("okp-empty-wordmark-ok");
    let wordmark_player = gtk::Label::new(Some(" Player"));
    wordmark_player.add_css_class("okp-empty-wordmark-player");
    wordmark.append(&wordmark_ok);
    wordmark.append(&wordmark_player);
    wordmark
}

fn welcome_actions(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    include_folder: bool,
) -> gtk::Box {
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-empty-actions");
    actions.set_halign(gtk::Align::Center);

    let open_button = gtk::Button::with_label("Open media");
    open_button.add_css_class("okp-empty-primary-button");
    let open_parent = parent.clone();
    let open_state = Rc::clone(&state);
    let open_toast = Rc::clone(&status_toast);
    open_button.connect_clicked(move |_| {
        open_media_dialog(&open_parent, Rc::clone(&open_state), Rc::clone(&open_toast))
    });
    actions.append(&open_button);

    if include_folder {
        let folder_button = gtk::Button::with_label("Open folder");
        folder_button.add_css_class("okp-empty-secondary-button");
        let folder_parent = parent.clone();
        let folder_state = Rc::clone(&state);
        let folder_toast = Rc::clone(&status_toast);
        folder_button.connect_clicked(move |_| {
            open_folder_dialog(
                &folder_parent,
                Rc::clone(&folder_state),
                Rc::clone(&folder_toast),
            );
        });
        actions.append(&folder_button);
    }

    let url_button = gtk::Button::with_label("Open URL");
    url_button.add_css_class("okp-empty-secondary-button");
    let url_parent = parent.clone();
    let url_state = Rc::clone(&state);
    let url_toast = Rc::clone(&status_toast);
    url_button.connect_clicked(move |_| {
        open_url_dialog(&url_parent, Rc::clone(&url_state), Rc::clone(&url_toast));
    });
    actions.append(&url_button);
    actions
}

fn welcome_hint() -> gtk::Label {
    let hint = gtk::Label::new(Some("Drop media here · press O to open"));
    hint.add_css_class("okp-empty-hint");
    hint.set_justify(gtk::Justification::Center);
    hint.set_wrap(true);
    hint.set_max_width_chars(40);
    hint
}

fn welcome_footer(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    private_session: bool,
) -> gtk::Box {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("okp-welcome-footer");

    let history = icon_text_button("document-open-recent-symbolic", "History");
    history.add_css_class("okp-welcome-footer-button");
    let history_parent = parent.clone();
    let history_state = Rc::clone(&state);
    let history_toast = Rc::clone(&status_toast);
    history.connect_clicked(move |_| {
        show_history_window(
            &history_parent,
            Rc::clone(&history_state),
            Rc::clone(&history_toast),
        );
    });
    footer.append(&history);

    let status = gtk::Label::new(Some(if private_session {
        "Private session · recents hidden"
    } else {
        "Recording history"
    }));
    status.add_css_class("okp-welcome-footer-status");
    status.set_hexpand(true);
    status.set_halign(gtk::Align::Center);
    footer.append(&status);

    let settings = gtk::Button::from_icon_name("emblem-system-symbolic");
    settings.add_css_class("okp-welcome-footer-button");
    settings.set_tooltip_text(Some("Settings"));
    let settings_parent = parent.clone();
    settings.connect_clicked(move |_| {
        open_settings_window(
            &settings_parent,
            Rc::clone(&state),
            Rc::clone(&status_toast),
        );
    });
    footer.append(&settings);
    footer
}

fn icon_text_button(icon_name: &str, text: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_has_frame(false);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(text)));
    button.set_child(Some(&content));
    button
}

pub(crate) fn show_history_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let window = captionless_transient_window(parent, "History", 760, 760, true);
    window.add_css_class("okp-history-window");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("okp-history-root");

    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("okp-history-page");
    page.set_margin_top(54);
    page.set_margin_end(28);
    page.set_margin_bottom(22);
    page.set_margin_start(28);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let heading = gtk::Label::new(Some("History"));
    heading.add_css_class("okp-history-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    header.append(&heading);

    let search = gtk::SearchEntry::new();
    search.add_css_class("okp-history-search");
    search.set_placeholder_text(Some("Search history"));
    search.set_width_chars(24);
    header.append(&search);
    page.append(&header);

    let subtitle = gtk::Label::new(Some("Everything you have opened, newest first."));
    subtitle.add_css_class("okp-history-subtitle");
    subtitle.set_xalign(0.0);
    page.append(&subtitle);

    if state.borrow().private_session {
        let banner = gtk::Label::new(Some(
            "Private session is on. Existing history is visible, but new opens are not recorded or resumed.",
        ));
        banner.add_css_class("okp-history-private-banner");
        banner.set_wrap(true);
        banner.set_xalign(0.0);
        page.append(&banner);
    }

    let result_caption = gtk::Label::new(None);
    result_caption.add_css_class("okp-history-result-caption");
    result_caption.set_xalign(0.0);
    page.append(&result_caption);

    let rows = gtk::Box::new(gtk::Orientation::Vertical, 5);
    rows.add_css_class("okp-history-rows");
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-history-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&rows));
    page.append(&scroller);

    rebuild_history_rows(&rows, &result_caption, &state, &window, &status_toast, "");
    let search_rows = rows.clone();
    let search_caption = result_caption.clone();
    let search_state = Rc::clone(&state);
    let search_window = window.clone();
    let search_toast = Rc::clone(&status_toast);
    search.connect_search_changed(move |entry| {
        rebuild_history_rows(
            &search_rows,
            &search_caption,
            &search_state,
            &search_window,
            &search_toast,
            entry.text().as_str(),
        );
    });

    root.append(&page);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&root));
    overlay.add_overlay(&captionless_window_drag_layer(&window));
    overlay.add_overlay(&settings_window_controls(&window));
    window.set_child(Some(&overlay));
    window.present();
}

fn rebuild_history_rows(
    rows: &gtk::Box,
    result_caption: &gtk::Label,
    state: &Rc<RefCell<PlayerState>>,
    window: &gtk::Window,
    status_toast: &Rc<StatusToast>,
    query: &str,
) {
    clear_box(rows);
    let items = state.borrow().history.search(query);
    let trimmed = query.trim();
    let caption = if trimmed.is_empty() {
        format!(
            "{} record{}",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} result{} for \"{}\"",
            items.len(),
            if items.len() == 1 { "" } else { "s" },
            trimmed
        )
    };
    result_caption.set_label(&caption);

    if items.is_empty() {
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 8);
        empty.add_css_class("okp-history-empty");
        let icon = gtk::Image::from_icon_name("document-open-recent-symbolic");
        icon.set_pixel_size(28);
        empty.append(&icon);
        let label = gtk::Label::new(Some(if trimmed.is_empty() {
            "Nothing here yet"
        } else {
            "No matching history"
        }));
        label.add_css_class("okp-history-empty-title");
        empty.append(&label);
        rows.append(&empty);
        return;
    }

    for item in items {
        rows.append(&history_row(&item, Rc::clone(state), window, status_toast));
    }
}

fn history_row(
    item: &HistoryItem,
    state: Rc<RefCell<PlayerState>>,
    window: &gtk::Window,
    status_toast: &Rc<StatusToast>,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-history-row");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(&item.path));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&history_thumbnail(item, 88, 50));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(&item.title));
    title.add_css_class("okp-history-row-title");
    title.set_xalign(0.0);
    title.set_width_chars(1);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);
    text.append(&title);

    let location = gtk::Label::new(Some(&item.location));
    location.add_css_class("okp-history-row-location");
    location.set_xalign(0.0);
    location.set_width_chars(1);
    location.set_hexpand(true);
    location.set_ellipsize(pango::EllipsizeMode::Middle);
    text.append(&location);

    let context = gtk::Label::new(Some(&format!(
        "{} · {}",
        item.state_label,
        recents_shelf::opened_context(item.updated_at_unix, unix_now())
    )));
    context.add_css_class(if item.state_kind == HistoryStateKind::Progress {
        "okp-history-row-progress-label"
    } else {
        "okp-history-row-context"
    });
    context.set_xalign(0.0);
    context.set_ellipsize(pango::EllipsizeMode::End);
    text.append(&context);

    if item.state_kind != HistoryStateKind::Finished {
        let progress = gtk::ProgressBar::new();
        progress.add_css_class("okp-history-row-progress");
        progress.set_fraction(item.progress);
        text.append(&progress);
    }
    row.append(&text);
    button.set_child(Some(&row));

    let path = PathBuf::from(&item.path);
    let close_window = window.clone();
    let toast = Rc::clone(status_toast);
    button.connect_clicked(move |_| {
        if !path.is_file() {
            toast.show("History file is no longer available");
            return;
        }
        close_window.close();
        load_media_path(&state, path.clone());
    });
    button
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
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
