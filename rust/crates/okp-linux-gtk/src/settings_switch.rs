use super::*;

pub(crate) const SETTINGS_SWITCH_WIDTH: i32 = 39;
pub(crate) const SETTINGS_SWITCH_HEIGHT: i32 = 22;
pub(crate) const SETTINGS_SWITCH_KNOB_SIZE: i32 = 16;

pub(crate) fn settings_switch_button(active: bool, accessible_label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-switch");
    button.set_has_frame(false);
    button.set_accessible_role(gtk::AccessibleRole::Switch);
    button.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    button.set_size_request(SETTINGS_SWITCH_WIDTH, SETTINGS_SWITCH_HEIGHT);
    button.set_halign(gtk::Align::End);
    button.set_valign(gtk::Align::Center);

    let knob = gtk::Box::new(gtk::Orientation::Vertical, 0);
    knob.add_css_class("okp-settings-switch-knob");
    knob.set_size_request(SETTINGS_SWITCH_KNOB_SIZE, SETTINGS_SWITCH_KNOB_SIZE);
    knob.set_valign(gtk::Align::Center);
    button.set_child(Some(&knob));
    set_settings_switch_active(&button, active);
    button
}

pub(crate) fn set_settings_switch_active(button: &gtk::Button, active: bool) {
    if active {
        button.add_css_class("is-active");
    } else {
        button.remove_css_class("is-active");
    }
    if let Some(knob) = button.first_child() {
        knob.set_halign(if active {
            gtk::Align::End
        } else {
            gtk::Align::Start
        });
    }
    button.update_state(&[gtk::accessible::State::Checked(if active {
        gtk::AccessibleTristate::True
    } else {
        gtk::AccessibleTristate::False
    })]);
}
