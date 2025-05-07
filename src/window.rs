use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::glib::clone;
use gtk::{gio, glib};

use crate::application::QuickShareApplication;
use crate::config::{APP_ID, PROFILE};
use crate::objects::file_transfer::{self, FileTransferObject, TransferKind};
use crate::tokio_runtime;

mod imp {
    use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

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
        pub receive_file_transfer_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub send_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub send_select_files_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub selected_files_card_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub selected_files_card_caption: TemplateChild<gtk::Label>,
        #[template_child]
        pub selected_files_card_cancel_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub send_file_transfer_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub loading_nearby_devices_spinner: TemplateChild<adw::Spinner>,

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

        #[default(gio::ListStore::new::<FileTransferObject>())]
        pub receive_file_transfer_model: gio::ListStore,
        #[default(gio::ListStore::new::<FileTransferObject>())]
        pub send_file_transfer_model: gio::ListStore,
        pub active_discovered_endpoints:
            Arc<tokio::sync::Mutex<HashMap<String, FileTransferObject>>>,
        pub active_file_requests: Arc<tokio::sync::Mutex<HashMap<String, FileTransferObject>>>,
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

                                let title = formatx!(
                                    &ngettext(
                                        "{} file is ready to send",
                                        "{} files are ready to send",
                                        files.n_items(),
                                    ),
                                    files.n_items()
                                )
                                .unwrap_or_default();

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

        let send_file_transfer_model = &imp.send_file_transfer_model;
        let send_file_transfer_listbox = imp.send_file_transfer_listbox.get();
        send_file_transfer_listbox.bind_model(
            Some(send_file_transfer_model),
            clone!(
                #[weak]
                imp,
                #[upgrade_or]
                adw::Bin::new().into(),
                move |obj| {
                    let model_item = obj.downcast_ref::<FileTransferObject>().unwrap();
                    create_file_transfer_card(&imp, model_item).into()
                }
            ),
        );

        let receive_file_transfer_model = &imp.receive_file_transfer_model;
        let receive_file_transfer_listbox = imp.receive_file_transfer_listbox.get();
        receive_file_transfer_listbox.bind_model(
            Some(receive_file_transfer_model),
            clone!(
                #[weak]
                imp,
                #[upgrade_or]
                adw::Bin::new().into(),
                move |obj| {
                    let model_item = obj.downcast_ref::<FileTransferObject>().unwrap();
                    create_file_transfer_card(&imp, model_item).into()
                }
            ),
        );

        fn create_file_transfer_card(
            imp: &imp::QuickShareApplicationWindow,
            file_transfer_state: &FileTransferObject,
        ) -> adw::Bin {
            // FIXME: UI for request pin code

            let (caption, title) = match dbg!(file_transfer_state.transfer_kind()) {
                TransferKind::Receive => {
                    let device_name = file_transfer::ChannelMessage::get_device_name(
                        &file_transfer_state.channel_message().0,
                    );

                    let file_count = file_transfer_state.filenames().len();
                    (
                        formatx!(
                            ngettext(
                                "{} wants to share {} file",
                                "{} wants to share {} files",
                                file_count as u32
                            ),
                            &device_name,
                            file_count
                        )
                        .unwrap_or_default(),
                        device_name,
                    )
                }
                TransferKind::Send => {
                    let device_name = file_transfer_state
                        .endpoint_info()
                        .name
                        .clone()
                        .unwrap_or(gettext("Unknown device").into());

                    let file_count = imp.selected_files_to_send.as_ref().borrow().len();
                    // FIXME: there's probably a better way to do this right?
                    // As it is, translators might have issues translating this due to loss of context
                    (
                        formatx!(
                            ngettext(
                                "Ready to share {} file to this device",
                                "Ready to share {} files to this device",
                                file_count as u32
                            ),
                            file_count
                        )
                        .unwrap_or_default(),
                        device_name,
                    )
                }
            };

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

            match file_transfer_state.transfer_kind() {
                TransferKind::Receive => {
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
                }
                TransferKind::Send => {
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
                }
            };

            adw::Bin::builder().child(&root_card_box).build()
        }

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
                    imp.rqs
                        .blocking_lock()
                        .as_mut()
                        .unwrap()
                        .change_visibility(rqs_lib::Visibility::Visible);
                } else {
                    receive_idle_status_page.set_title(&gettext("Not ready to receive"));
                    receive_idle_status_page.set_icon_name(Some("network-offline-symbolic"));
                    receive_idle_status_page.set_description(Some(&gettext(
                        "No longer broadcasting this device as available",
                    )));
                    imp.rqs
                        .blocking_lock()
                        .as_mut()
                        .unwrap()
                        .change_visibility(rqs_lib::Visibility::Invisible);
                }
            }
        ));
    }

    fn setup_rqs_service(&self) {
        let imp = self.imp();

        let (tx, rx) = async_channel::bounded(1);
        tokio_runtime().spawn(async move {
            let download_path = directories::UserDirs::new()
                .unwrap()
                .download_dir()
                .unwrap()
                .to_path_buf();

            tracing::info!(?download_path, "Starting RQS service");

            // FIXME: Allow setting a const port number in app preferences
            // and, download_path
            let mut rqs =
                rqs_lib::RQS::new(rqs_lib::Visibility::Visible, None, Some(download_path));

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
            let (tx, rx) = async_channel::bounded(1);
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
                                    let name = file_transfer::ChannelMessage::get_device_name(
                                        &channel_message,
                                    );

                                    // FIXME: Not reacting to cancellation of transfer request by the sender itself
                                    // We don't receive a cancelation message when the other device has cancelled
                                    // their own request until the user "accepts" the request, after which the
                                    // accept request gets rejected and then we get the cancellation message.
                                    // But this is not how it's supposed to be, first party quickshare clients
                                    // are able to figure out the cancellation, just not here... something's
                                    // up in the lib.
                                    //
                                    // Lib has access to the event but it's not being exposed
                                    // [log] rqs_lib::hdl::inbound: Transfer canceled

                                    tracing::info!(
                                        ?channel_message,
                                        "{name} wants to start a transfer"
                                    );
                                    tx.send(channel_message).await.unwrap();

                                    // FIXME: Send desktop notification aswell
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
            glib::spawn_future_local(clone!(
                #[weak]
                imp,
                async move {
                    loop {
                        {
                            let channel_message = rx.recv().await.unwrap();

                            let mut active_file_requests = imp.active_file_requests.lock().await;
                            if let Some(file_transfer) =
                                active_file_requests.get(&channel_message.id)
                            {
                                // FIXME: Listen to channel_message updates
                                // and update the UI accordingly

                                // Update file request state
                                file_transfer.set_filenames(
                                    channel_message
                                        .meta
                                        .as_ref()
                                        .unwrap()
                                        .files
                                        .as_ref()
                                        .unwrap()
                                        .clone(),
                                );
                                file_transfer.set_channel_message(file_transfer::ChannelMessage(
                                    channel_message,
                                ));
                            } else {
                                // Add new file request
                                let obj = FileTransferObject::new(TransferKind::Receive);
                                let id = channel_message.id.clone();
                                obj.set_filenames(
                                    channel_message
                                        .meta
                                        .as_ref()
                                        .unwrap()
                                        .files
                                        .as_ref()
                                        .unwrap()
                                        .clone(),
                                );
                                obj.set_channel_message(file_transfer::ChannelMessage(
                                    channel_message,
                                ));
                                imp.receive_file_transfer_model.append(&obj);
                                active_file_requests.insert(id, obj);
                            }
                        }

                        if imp.receive_file_transfer_model.n_items() == 0 {
                            imp.receive_stack
                                .set_visible_child_name("receive_idle_status_page");
                        } else {
                            imp.receive_stack
                                .set_visible_child_name("receive_request_page");
                        }
                    }
                }
            ));

            // MDNS discovery receiver
            // Discover the devices to send file transfer requests to
            // The Sender used in RQS::discovery()
            let (tx, rx) = async_channel::bounded(1);
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

                    // FIXME: Start this when a file is selected for the first time?
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
                                tracing::trace!(?endpoint_info, "Processing endpoint");
                                tx.send(endpoint_info).await.unwrap();
                            }
                            Err(err) => {
                                tracing::error!(%err,"MDNS discovery error");
                            }
                        }
                    }
                }
            ));
            glib::spawn_future_local(clone!(
                #[weak]
                imp,
                async move {
                    loop {
                        {
                            let endpoint_info = rx.recv().await.unwrap();

                            let mut active_discovered_endpoints =
                                imp.active_discovered_endpoints.lock().await;
                            if let Some(file_transfer) =
                                active_discovered_endpoints.get(&endpoint_info.id)
                            {
                                if endpoint_info.present.is_none() {
                                    // Endpoint disconnected, remove endpoint
                                    if let Some(pos) =
                                        imp.send_file_transfer_model.find(file_transfer)
                                    {
                                        tracing::info!(?endpoint_info, "Disconnected endpoint");
                                        imp.send_file_transfer_model.remove(pos);
                                    }
                                    active_discovered_endpoints.remove(&endpoint_info.id);
                                } else {
                                    // Update endpoint
                                    tracing::info!(?endpoint_info, "Updated endpoint info");
                                    file_transfer.set_endpoint_info(file_transfer::EndpointInfo(
                                        endpoint_info,
                                    ));

                                    // FIXME: Listen to endpoint_info updates
                                    // and update the UI accordingly
                                }
                            } else {
                                // Set new endpoint
                                tracing::info!(?endpoint_info, "Connected endpoint");
                                let obj = FileTransferObject::new(TransferKind::Send);
                                let id = endpoint_info.id.clone();
                                obj.set_endpoint_info(file_transfer::EndpointInfo(endpoint_info));
                                imp.send_file_transfer_model.append(&obj);
                                active_discovered_endpoints.insert(id, obj);
                            }
                        }

                        let loading_nearby_devices_spinner =
                            imp.loading_nearby_devices_spinner.get();
                        if imp.send_file_transfer_model.n_items() == 0 {
                            loading_nearby_devices_spinner.set_visible(true);
                        } else {
                            loading_nearby_devices_spinner.set_visible(false);
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
