use okp_core::AppIdentity;
use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::prelude::*;
use velopack::VelopackApp;

struct AppModel {
    identity: AppIdentity,
}

#[derive(Debug)]
enum AppMsg {}

#[relm4::component]
impl SimpleComponent for AppModel {
    type Init = AppIdentity;
    type Input = AppMsg;
    type Output = ();

    view! {
        gtk::ApplicationWindow {
            set_title: Some(&model.identity.name),
            set_default_width: 1120,
            set_default_height: 680,

            gtk::Overlay {
                add_css_class: "okp-root",

                gtk::Box {
                    set_hexpand: true,
                    set_vexpand: true,
                    add_css_class: "okp-video-plane",
                }
            }
        }
    }

    fn init(
        identity: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self { identity };
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }
}

fn main() {
    VelopackApp::build().set_auto_apply_on_startup(false).run();

    let app = RelmApp::new("com.befeast.okplayer");
    app.run::<AppModel>(AppIdentity::linux());
}
