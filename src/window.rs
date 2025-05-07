use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib::clone;
use gtk::{gio, glib};

use crate::application::QuickShareApplication;
use crate::config::{APP_ID, PROFILE};
use crate::tokio_runtime;

mod imp {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Debug, gtk::CompositeTemplate, better_default::Default)]
    #[template(resource = "/io/github/nozwock/QuickShare/ui/window.ui")]
    pub struct QuickShareApplicationWindow {
        #[default(gio::Settings::new(APP_ID))]
        pub settings: gio::Settings,

        #[template_child]
        pub root_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub transfer_kind_view_stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub transfer_kind_nav_view: TemplateChild<adw::NavigationView>,

        #[template_child]
        pub receive_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub receive_request_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub send_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub send_select_files_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub nearby_devices_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub selected_files_card_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub selected_files_card_caption: TemplateChild<gtk::Label>,
        #[template_child]
        pub selected_files_card_cancel_button: TemplateChild<gtk::Button>,

        #[template_child]
        pub device_name_label: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub device_visibility_switch: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub receive_idle_status_page: TemplateChild<adw::StatusPage>,

        #[template_child]
        pub test_cycle_pages_button: TemplateChild<gtk::Button>,

        pub rqs: Arc<tokio::sync::Mutex<Option<rqs_lib::RQS>>>,
        pub file_sender: RefCell<Option<tokio::sync::mpsc::Sender<rqs_lib::SendInfo>>>,
        pub ble_receiver: RefCell<Option<tokio::sync::broadcast::Receiver<()>>>,
        pub mdns_discovery_broadcast_tx:
            Arc<tokio::sync::Mutex<Option<tokio::sync::broadcast::Sender<rqs_lib::EndpointInfo>>>>,

        pub selected_files_to_send: Rc<RefCell<Vec<PathBuf>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QuickShareApplicationWindow {
        const NAME: &'static str = "QuickShareApplicationWindow";
        type Type = super::QuickShareApplicationWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        // You must call `Widget`'s `init_template()` within `instance_init()`.
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for QuickShareApplicationWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            // Devel Profile
            if PROFILE == "Devel" {
                obj.add_css_class("devel");
            }

            // Load latest window state
            obj.load_window_size();
            obj.setup_ui();
            obj.setup_rqs_service();
        }
    }

    impl WidgetImpl for QuickShareApplicationWindow {}
    impl WindowImpl for QuickShareApplicationWindow {
        // Save window state on delete event
        fn close_request(&self) -> glib::Propagation {
            if let Err(err) = self.obj().save_window_size() {
                tracing::warn!("Failed to save window state, {}", &err);
            }

            let (tx, rx) = async_channel::bounded(1);
            tokio_runtime().spawn(clone!(
                #[weak(rename_to = rqs)]
                self.rqs,
                async move {
                    rqs.lock().await.as_mut().unwrap().stop().await;
                    tracing::info!("Stopped RQS service");
                    tx.send(()).await.unwrap();
                }
            ));

            rx.recv_blocking().unwrap();

            // Pass close request on to the parent
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for QuickShareApplicationWindow {}
    impl AdwApplicationWindowImpl for QuickShareApplicationWindow {}
}

glib::wrapper! {
    pub struct QuickShareApplicationWindow(ObjectSubclass<imp::QuickShareApplicationWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionMap, gio::ActionGroup, gtk::Root;
}

impl QuickShareApplicationWindow {
    pub fn new(app: &QuickShareApplication) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let imp = self.imp();

        let (width, height) = self.default_size();

        imp.settings.set_int("window-width", width)?;
        imp.settings.set_int("window-height", height)?;

        imp.settings
            .set_boolean("is-maximized", self.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let imp = self.imp();

        let width = imp.settings.int("window-width");
        let height = imp.settings.int("window-height");
        let is_maximized = imp.settings.boolean("is-maximized");

        self.set_default_size(width, height);

        if is_maximized {
            self.maximize();
        }
    }

    fn setup_ui(&self) {
        let imp = self.imp();

        // imp.transfer_kind_nav_view.get().push_by_tag("transfer_history_page");

        // FIXME: Keep the device name stored as preference and restore it on app start
        let device_name_label = imp.device_name_label.get();
        device_name_label.set_subtitle(&whoami::devicename());

        // FIXME: implement send page's select drop zone
        imp.send_select_files_button.connect_clicked(clone!(
            #[weak]
            imp,
            move |_| {
                gtk::FileDialog::new().open_multiple(
                    imp.obj()
                        .root()
                        .and_downcast_ref::<adw::ApplicationWindow>(),
                    None::<&gio::Cancellable>,
                    move |files| {
                        // FIXME: abstract this away into a reusable function
                        // since this'll be called from both the button and drop zone
                        // Maybe have it as a action later

                        if let Ok(files) = files {
                            if files.n_items() == 0 {
                                // FIXME: Show toast about not being able to access files
                            } else {
                                // FIXME: Start MDNS discovery here once a file is selected for the first time?
                                imp.send_stack
                                    .get()
                                    .set_visible_child_name("send_nearby_devices_page");

                                let title_fmt = ngettext(
                                    "One file is ready to send",
                                    "files are ready to send",
                                    files.n_items(),
                                );
                                let title = if files.n_items() > 1 {
                                    format!("{} {}", files.n_items(), title_fmt)
                                } else {
                                    title_fmt
                                };

                                imp.selected_files_card_title.get().set_label(&title);

                                imp.selected_files_to_send.as_ref().borrow_mut().clear();
                                for i in 0..files.n_items() {
                                    let file =
                                        files.item(i).unwrap().downcast::<gio::File>().unwrap();

                                    tracing::info!(file = ?file.path(), "Selected file");
                                    if let Some(path) = file.path() {
                                        imp.selected_files_to_send.as_ref().borrow_mut().push(path);
                                    }
                                }

                                imp.selected_files_card_caption.get().set_label(
                                    &imp.selected_files_to_send
                                        .as_ref()
                                        .borrow()
                                        .iter()
                                        .map(|it| {
                                            it.file_name().and_then(|it| Some(it.to_string_lossy()))
                                        })
                                        .flatten()
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                );
                            }
                        };
                    },
                );
            }
        ));
        imp.selected_files_card_cancel_button
            .connect_clicked(clone!(
                #[weak]
                imp,
                move |_| {
                    imp.send_stack
                        .get()
                        .set_visible_child_name("send_select_files_status_page");
                    imp.selected_files_to_send.as_ref().borrow_mut().clear();
                }
            ));

        // FIXME: remove test code
        let receive_stack = imp.receive_stack.get();
        let send_stack = imp.send_stack.get();
        imp.test_cycle_pages_button.get().connect_clicked(clone!(
            #[weak]
            imp,
            #[weak]
            receive_stack,
            #[weak]
            send_stack,
            move |_| {
                match imp
                    .transfer_kind_view_stack
                    .get()
                    .visible_child_name()
                    .unwrap()
                    .as_str()
                {
                    "receive" => {
                        if receive_stack.visible_child_name().unwrap() == "receive_idle_status_page"
                        {
                            receive_stack.set_visible_child_name("receive_request_page");
                        } else {
                            receive_stack.set_visible_child_name("receive_idle_status_page");
                        }
                    }
                    "send" => {
                        if send_stack.visible_child_name().unwrap()
                            == "send_select_files_status_page"
                        {
                            send_stack.set_visible_child_name("send_nearby_devices_page");
                        } else {
                            send_stack.set_visible_child_name("send_select_files_status_page");
                        }
                    }
                    _ => {
                        unreachable!();
                    }
                };
            }
        ));

        fn icon_info_card(title: &str, caption: &str) -> gtk::Box {
            // `card` style will be applied with `boxed-list-separate` on ListBox
            let root_card_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                // v/h-align would prevent the card from expanding when space is available
                // .valign(gtk::Align::Center)
                // .halign(gtk::Align::Center)
                // .css_classes(["card"])
                .build();

            let main_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_start(18)
                .margin_end(18)
                .margin_top(18)
                .margin_bottom(18)
                .spacing(12)
                .build();
            root_card_box.append(&main_box);

            let top_box = gtk::Box::builder().spacing(18).build();
            main_box.append(&top_box);

            // `object-select-symbolic` for success status icon
            let device_icon_image = adw::Avatar::builder()
                .icon_name("preferences-system-network-symbolic")
                .size(48)
                .build();
            top_box.append(&device_icon_image);

            let label_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .build();
            top_box.append(&label_box);

            let title_label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .label(title)
                .css_classes(["title-4"])
                .build();
            let caption_label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .label(caption)
                .css_classes(["caption"])
                .build();
            label_box.append(&title_label);
            label_box.append(&caption_label);

            root_card_box
        }

        fn share_request_card(title: &str, caption: &str) -> gtk::Box {
            // FIXME: UI for request pin code
            let root_card_box = icon_info_card(title, caption);
            let main_box = root_card_box
                .first_child()
                .and_downcast::<gtk::Box>()
                .unwrap();

            let button_box = gtk::Box::builder()
                // Let the buttons expand, they look weird when always compact,
                // leads to too much empty space in the card
                // .halign(gtk::Align::Center)
                .spacing(12)
                .build();
            main_box.append(&button_box);

            let decline_button = gtk::Button::builder()
                .hexpand(true)
                .can_shrink(false)
                .label(gettext("Decline"))
                .css_classes(["pill"])
                .build();
            let accept_button = gtk::Button::builder()
                .hexpand(true)
                .can_shrink(false)
                .label(gettext("Accept"))
                .css_classes(["pill", "suggested-action"])
                .build();
            button_box.append(&decline_button);
            button_box.append(&accept_button);

            root_card_box
        }

        fn send_to_nearby_device_card(title: &str, caption: &str) -> gtk::Box {
            let root_card_box = icon_info_card(title, caption);
            let main_box = root_card_box
                .first_child()
                .and_downcast::<gtk::Box>()
                .unwrap();

            let button_box = gtk::Box::builder()
                // Let the buttons expand, they look weird when always compact,
                // leads to too much empty space in the card
                // .halign(gtk::Align::Center)
                .spacing(12)
                .build();
            main_box.append(&button_box);

            let accept_button = gtk::Button::builder()
                .hexpand(true)
                .can_shrink(false)
                .label(gettext("Send"))
                .css_classes(["pill", "suggested-action"])
                .build();
            button_box.append(&accept_button);

            root_card_box
        }

        // FIXME: remove test code
        imp.receive_request_listbox
            .get()
            .append(&share_request_card("Device 1", "Wants to share 4 files"));
        imp.receive_request_listbox
            .get()
            .append(&share_request_card("Device 3", "Wants to share 2 files"));

        imp.nearby_devices_listbox
            .get()
            .append(&send_to_nearby_device_card(
                "Device 1",
                "Send selected files to Device 1",
            ));
        imp.nearby_devices_listbox
            .get()
            .append(&send_to_nearby_device_card(
                "Device 3",
                "Send selected files to Device 3",
            ));

        let device_visibility_switch = imp.device_visibility_switch.get();
        device_visibility_switch.connect_active_notify(clone!(
            #[weak]
            imp,
            move |obj| {
                let receive_idle_status_page = imp.receive_idle_status_page.get();
                if obj.is_active() {
                    receive_idle_status_page.set_title(&gettext("Ready to receive"));
                    receive_idle_status_page.set_icon_name(Some("network-receive-symbolic"));
                    receive_idle_status_page
                        .set_description(Some(&gettext("Waiting for share requests")));
                } else {
                    receive_idle_status_page.set_title(&gettext("Not ready to receive"));
                    receive_idle_status_page.set_icon_name(Some("network-offline-symbolic"));
                    receive_idle_status_page.set_description(None);
                }
            }
        ));
    }

    fn setup_rqs_service(&self) {
        let imp = self.imp();

        let (tx, rx) = async_channel::bounded(1);
        tokio_runtime().spawn(async move {
            tracing::info!("Starting RQS service");

            // FIXME: Allow setting a const port number in app preferences
            // and, download_path
            let mut rqs = rqs_lib::RQS::new(
                rqs_lib::Visibility::Visible,
                None,
                Some(PathBuf::from("/tmp")),
            );

            let (file_sender, ble_receiver) = rqs.run().await.unwrap();

            tx.send((rqs, file_sender, ble_receiver)).await.unwrap();
        });
        glib::spawn_future_local(clone!(
            #[weak]
            imp,
            async move {
                let (rqs, file_sender, ble_receiver) = rx.recv().await.unwrap();
                *imp.rqs.lock().await = Some(rqs);
                *imp.file_sender.borrow_mut() = Some(file_sender);
                *imp.ble_receiver.borrow_mut() = Some(ble_receiver);

                let (mdns_discovery_broadcast_tx, _) =
                    tokio::sync::broadcast::channel::<rqs_lib::EndpointInfo>(10);
                *imp.mdns_discovery_broadcast_tx.lock().await = Some(mdns_discovery_broadcast_tx);

                tracing::debug!("Fetched RQS instance after run()");

                imp.device_visibility_switch.set_active(true);
                imp.root_stack.get().set_visible_child_name("main_page");

                spawn_rqs_receiver_tasks(&imp);
            }
        ));

        fn spawn_rqs_receiver_tasks(imp: &imp::QuickShareApplicationWindow) {
            tokio_runtime().spawn(clone!(
                #[weak(rename_to = rqs)]
                imp.rqs,
                async move {
                    let mut rx = rqs
                        .lock()
                        .await
                        .as_ref()
                        .expect("State must be set")
                        .message_sender
                        .subscribe();

                    loop {
                        match rx.recv().await {
                            Ok(channel_message) => {
                                if channel_message
                                    .state
                                    .as_ref()
                                    .unwrap_or(&rqs_lib::State::Initial)
                                    == &rqs_lib::State::WaitingForUserConsent
                                {
                                    let name = channel_message
                                        .meta
                                        .as_ref()
                                        .and_then(|meta| meta.source.as_ref())
                                        .map(|source| source.name.clone())
                                        .unwrap_or_else(|| "Unknown".to_string());

                                    tracing::info!(
                                        ?channel_message,
                                        "{name} wants to start a transfer"
                                    );

                                    // FIXME:
                                    // send_request_notification(name, channel_msg.id.clone(), &capp_handle);
                                }
                            }
                            Err(err) => {
                                tracing::error!(%err)
                            }
                        };
                    }
                }
            ));

            // MDNS discovery receiver
            // The Sender used in RQS::discovery()
            tokio_runtime().spawn(clone!(
                #[weak(rename_to = mdns_discovery_broadcast_tx)]
                imp.mdns_discovery_broadcast_tx,
                #[weak(rename_to = rqs)]
                imp.rqs,
                async move {
                    let mdns_discovery_broadcast_tx = mdns_discovery_broadcast_tx
                        .lock()
                        .await
                        .as_ref()
                        .unwrap()
                        .clone();
                    let mut mdns_discovery_rx = mdns_discovery_broadcast_tx.subscribe();

                    // FIXME: Start this when a file is selected for the first time
                    // Start discovery
                    rqs.lock()
                        .await
                        .as_mut()
                        .unwrap()
                        .discovery(mdns_discovery_broadcast_tx)
                        .unwrap();

                    loop {
                        match mdns_discovery_rx.recv().await {
                            Ok(endpoint_info) => {
                                // FIXME: Handle discovered devices to share files to
                                tracing::debug!(?endpoint_info);
                            }
                            Err(err) => {
                                tracing::error!(%err,"MDNS discovery error");
                            }
                        }
                    }
                }
            ));

            tokio_runtime().spawn(clone!(
                #[weak(rename_to = rqs)]
                imp.rqs,
                async move {
                    let mut visibility_receiver = rqs
                        .lock()
                        .await
                        .as_ref()
                        .expect("State must be set")
                        .visibility_sender
                        .lock()
                        .unwrap()
                        .subscribe();

                    loop {
                        match visibility_receiver.changed().await {
                            Ok(_) => {
                                // FIXME: update visibility in UI?
                                let visibility = visibility_receiver.borrow_and_update();
                                tracing::debug!(?visibility, "Visibility change");
                            }
                            Err(err) => {
                                tracing::error!(%err,"Visibility watcher error");
                            }
                        }
                    }
                }
            ));

            let mut ble_receiver = imp.ble_receiver.borrow().as_ref().unwrap().resubscribe();
            tokio_runtime().spawn(async move {
                // let mut last_sent = std::time::Instant::now() - std::time::Duration::from_secs(120);
                loop {
                    match ble_receiver.recv().await {
                        Ok(_) => {
                            // let is_visible = device_visibility_switch.is_active();
                            // FIXME: get visibility via a channel
                            // and temporarily make device visible

                            tracing::debug!("Received BLE")
                        }
                        Err(err) => {
                            tracing::error!(%err,"Error receiving BLE");
                        }
                    }
                }
            });
        }
    }
}
