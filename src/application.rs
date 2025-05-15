use gettextrs::gettext;
use tracing::{debug, info};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib};

use crate::config::{APP_ID, PKGDATADIR, PROFILE, VERSION};
use crate::window::PacketApplicationWindow;

mod imp {
    use super::*;
    use glib::WeakRef;
    use std::cell::OnceCell;

    #[derive(Debug, Default)]
    pub struct PacketApplication {
        pub window: OnceCell<WeakRef<PacketApplicationWindow>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PacketApplication {
        const NAME: &'static str = "PacketApplication";
        type Type = super::PacketApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for PacketApplication {}

    impl ApplicationImpl for PacketApplication {
        fn activate(&self) {
            debug!("GtkApplication<PacketApplication>::activate");
            self.parent_activate();
            let app = self.obj();

            if let Some(window) = self.window.get() {
                let window = window.upgrade().unwrap();
                window.present();
                return;
            }

            let window = PacketApplicationWindow::new(&app);
            self.window
                .set(window.downgrade())
                .expect("Window already set.");

            app.main_window().present();
        }

        fn startup(&self) {
            debug!("GtkApplication<PacketApplication>::startup");
            self.parent_startup();
            let app = self.obj();

            // Set icons for shell
            gtk::Window::set_default_icon_name(APP_ID);

            app.setup_css();
            app.setup_gactions();
            app.setup_accels();
        }
    }

    impl GtkApplicationImpl for PacketApplication {}
    impl AdwApplicationImpl for PacketApplication {}
}

glib::wrapper! {
    pub struct PacketApplication(ObjectSubclass<imp::PacketApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionMap, gio::ActionGroup;
}

impl PacketApplication {
    fn main_window(&self) -> PacketApplicationWindow {
        self.imp().window.get().unwrap().upgrade().unwrap()
    }

    fn setup_gactions(&self) {
        // Quit
        let action_quit = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| {
                // This is needed to trigger the delete event and saving the window state
                app.main_window().close();
                app.quit();
            })
            .build();

        // About
        let action_about = gio::ActionEntry::builder("about")
            .activate(|app: &Self, _, _| {
                app.show_about_dialog();
            })
            .build();
        self.add_action_entries([action_quit, action_about]);
    }

    // Sets up keyboard shortcuts
    fn setup_accels(&self) {
        self.set_accels_for_action("app.quit", &["<Control>q"]);
        self.set_accels_for_action("window.close", &["<Control>w"]);
        self.set_accels_for_action("win.preferences", &["<Control>comma"]);
    }

    fn setup_css(&self) {
        let provider = gtk::CssProvider::new();
        provider.load_from_resource("/io/github/nozwock/Packet/style.css");
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    #[allow(dead_code)]
    fn authors() -> Vec<&'static str> {
        // Authors are defined in Cargo.toml
        env!("CARGO_PKG_AUTHORS").split(":").collect()
    }

    fn show_about_dialog(&self) {
        // Reference:
        // https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.7/class.AboutDialog.html
        // https://github.com/youpie/Iconic/blob/main/src/application.rs
        let dialog = adw::AboutDialog::builder()
            .application_name(gettext("Packet"))
            .application_icon(APP_ID)
            .version(VERSION)
            .developer_name("nozwock")
            // format: "Name https://example.com" or "Name <email@example.com>"
            .developers(["nozwock https://github.com/nozwock"])
            .designers(["Dominik Baran https://gitlab.gnome.org/wallaby"])
            .license_type(gtk::License::Gpl30)
            .issue_url("https://github.com/nozwock/packet/issues")
            .website("https://github.com/nozwock/packet")
            .translator_credits(gettext("translator-credits"))
            .build();

        dialog.add_acknowledgement_section(
            Some(&gettext("Similar Projects")),
            &[
                "NearDrop https://github.com/grishka/NearDrop/",
                "rquickshare https://github.com/Martichou/rquickshare/",
            ],
        );

        dialog.present(Some(&self.main_window()));
    }

    pub fn run(&self) -> glib::ExitCode {
        info!("Packet ({})", APP_ID);
        info!("Version: {} ({})", VERSION, PROFILE);
        info!("Datadir: {}", PKGDATADIR);

        ApplicationExtManual::run(self)
    }
}

impl Default for PacketApplication {
    fn default() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("resource-base-path", "/io/github/nozwock/Packet/")
            .build()
    }
}
