use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::glib::clone;
use gtk::{gdk, gio, glib};
use rqs_lib::channel::ChannelMessage;

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
        pub send_drop_files_bin: TemplateChild<adw::Bin>,
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
        pub loading_nearby_devices_box: TemplateChild<gtk::Box>,

        #[template_child]
        pub device_name_label: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub device_visibility_switch: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub receive_idle_status_page: TemplateChild<adw::StatusPage>,

        pub rqs: Arc<tokio::sync::Mutex<Option<rqs_lib::RQS>>>,
        pub file_sender:
            Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<rqs_lib::SendInfo>>>>,
        pub ble_receiver: Rc<RefCell<Option<tokio::sync::broadcast::Receiver<()>>>>,
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

        let files_drop_target = gtk::DropTarget::builder()
            .name("files-drop-target")
            .actions(gdk::DragAction::COPY)
            .formats(&gdk::ContentFormats::for_type(gdk::FileList::static_type()))
            .build();
        imp.send_drop_files_bin
            .get()
            .add_controller(files_drop_target.clone());

        files_drop_target.connect_drop(clone!(
            #[weak]
            imp,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                if let Ok(file_list) = value.get::<gdk::FileList>() {
                    select_files_to_send_cb(&imp, file_list.files());
                }

                true
            }
        ));
        // imp.transfer_kind_nav_view.get().push_by_tag("transfer_history_page");

        // FIXME: Make device name configurable (at any time preferably) on rqs_lib side
        // Keep the device name stored as preference and restore it on app start
        let device_name_label = imp.device_name_label.get();
        device_name_label.set_subtitle(&whoami::devicename());

        fn select_files_to_send_cb(imp: &imp::QuickShareApplicationWindow, files: Vec<gio::File>) {
            if files.len() == 0 {
                // FIXME: Show toast about not being able to access files
            } else {
                imp.send_stack
                    .get()
                    .set_visible_child_name("send_nearby_devices_page");

                let title = formatx!(
                    &ngettext(
                        "{} file is ready to send",
                        "{} files are ready to send",
                        files.len() as u32,
                    ),
                    files.len()
                )
                .unwrap_or_default();

                imp.selected_files_card_title.get().set_label(&title);

                imp.selected_files_to_send.as_ref().borrow_mut().clear();
                for file in &files {
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
                        .map(|it| it.file_name().and_then(|it| Some(it.to_string_lossy())))
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(", "),
                );

                // Start MDNS Discovery
                tokio_runtime().spawn(clone!(
                    #[weak(rename_to = mdns_discovery_broadcast_tx)]
                    imp.mdns_discovery_broadcast_tx,
                    #[weak(rename_to = rqs)]
                    imp.rqs,
                    async move {
                        _ = rqs
                            .lock()
                            .await
                            .as_mut()
                            .unwrap()
                            .discovery(
                                mdns_discovery_broadcast_tx
                                    .lock()
                                    .await
                                    .as_ref()
                                    .unwrap()
                                    .clone(),
                            )
                            .inspect_err(|err| tracing::error!(%err));
                    }
                ));
            }
        }

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
                        if let Ok(files) = files {
                            let mut files_vec = Vec::with_capacity(files.n_items() as usize);
                            for i in 0..files.n_items() {
                                let file = files.item(i).unwrap().downcast::<gio::File>().unwrap();
                                files_vec.push(file);
                            }

                            select_files_to_send_cb(&imp, files_vec);
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

                    // Stop MDNS Discovery
                    tokio_runtime().spawn(clone!(
                        #[weak(rename_to = rqs)]
                        imp.rqs,
                        async move {
                            rqs.lock().await.as_mut().unwrap().stop_discovery();
                        }
                    ));

                    // Clear all cards
                    imp.send_file_transfer_model.remove_all();
                    imp.active_discovered_endpoints.blocking_lock().clear();

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
        send_file_transfer_model.connect_items_changed(clone!(
            #[weak]
            imp,
            move |model, _, _, _| {
                let loading_nearby_devices_box = imp.loading_nearby_devices_box.get();
                if model.n_items() == 0 {
                    loading_nearby_devices_box.set_visible(true);
                } else {
                    loading_nearby_devices_box.set_visible(false);
                }
            }
        ));

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
            model_item: &FileTransferObject,
        ) -> adw::Bin {
            let (caption, title) = match model_item.transfer_kind() {
                TransferKind::Receive => {
                    let device_name = file_transfer::ChannelMessage::get_device_name(
                        &model_item.channel_message().0,
                    );

                    let caption = if let Some(files) = model_item.channel_message().get_filenames()
                    {
                        formatx!(
                            ngettext(
                                "This device wants to share {} file",
                                "This device wants to share {} files",
                                files.len() as u32
                            ),
                            files.len()
                        )
                        .unwrap_or_default()
                    } else {
                        formatx!(
                            gettext("This device wants to share <i>\"{}\"</i>"),
                            model_item
                                .channel_message()
                                .get_text_data()
                                .unwrap()
                                .description
                        )
                        .unwrap_or_default()
                    };

                    (caption, device_name)
                }
                TransferKind::Send => {
                    let device_name = model_item
                        .endpoint_info()
                        .name
                        .clone()
                        .unwrap_or(gettext("Unknown device").into());

                    let file_count = imp.selected_files_to_send.as_ref().borrow().len();
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

            // FIXME: UI for request transfer pin code
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
                .wrap(true)
                .label(title)
                .css_classes(["title-4"])
                .build();
            let caption_label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .wrap(true)
                .use_markup(true)
                .label(caption)
                .css_classes(["caption"])
                .build();
            let result_label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .wrap(true)
                .visible(false)
                .css_classes(["caption"])
                .build();
            label_box.append(&title_label);
            label_box.append(&caption_label);
            label_box.append(&result_label);

            let button_box = gtk::Box::builder()
                // Let the buttons expand, they look weird when always compact,
                // leads to too much empty space in the card
                // .halign(gtk::Align::Center)
                .spacing(12)
                .build();
            main_box.append(&button_box);

            let id = match model_item.transfer_kind() {
                TransferKind::Receive => model_item.channel_message().id.clone(),
                TransferKind::Send => model_item.endpoint_info().id.clone(),
            };

            // FIXME: rqs_lib doesn't clean up broken downloads (whether due to user cancelation or otherwise)
            // so go fix it...
            let cancel_transfer_button = gtk::Button::builder()
                .hexpand(true)
                .label(gettext("Cancel"))
                .css_classes(["pill"])
                .visible(false)
                .build();
            cancel_transfer_button.connect_clicked(clone!(
                #[weak(rename_to = rqs)]
                imp.rqs,
                #[strong]
                id,
                move |button| {
                    // FIXME: Immediately change the UI to cancelled state
                    // or keep the current behaviour of making the button insensitive
                    // after one click
                    button.set_sensitive(false);
                    rqs.blocking_lock()
                        .as_mut()
                        .unwrap()
                        .message_sender
                        .send(ChannelMessage {
                            id: id.clone(),
                            action: Some(rqs_lib::channel::ChannelAction::CancelTransfer),
                            ..Default::default()
                        })
                        .unwrap();
                }
            ));

            match model_item.transfer_kind() {
                TransferKind::Receive => {
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
                    button_box.append(&cancel_transfer_button);

                    decline_button.connect_clicked(clone!(
                        #[weak(rename_to = rqs)]
                        imp.rqs,
                        #[strong]
                        id,
                        move |_| {
                            rqs.blocking_lock()
                                .as_mut()
                                .unwrap()
                                .message_sender
                                .send(ChannelMessage {
                                    id: id.clone(),
                                    action: Some(rqs_lib::channel::ChannelAction::RejectTransfer),
                                    ..Default::default()
                                })
                                .unwrap();
                        }
                    ));
                    accept_button.connect_clicked(clone!(
                        #[weak(rename_to = rqs)]
                        imp.rqs,
                        #[strong]
                        id,
                        move |_| {
                            rqs.blocking_lock()
                                .as_mut()
                                .unwrap()
                                .message_sender
                                .send(ChannelMessage {
                                    id: id.clone(),
                                    action: Some(rqs_lib::channel::ChannelAction::AcceptTransfer),
                                    ..Default::default()
                                })
                                .unwrap();
                        }
                    ));

                    // FIXME: Add new model properties like `title`, `caption`, `card_state`
                    // and so the ui can be updated by setting this properties outside of the UI
                    // code section, while we listen to property changes here
                    // And, this way the UI can be easily reproduced as well based on the model state
                    // unlike here. This is important for a transfer history page since that page
                    // will be built out of a list based on these model states
                    // Or,
                    // if possible via ListStore, just copy the widget instead of going model -> widget
                    model_item.connect_channel_message_notify(clone!(
                        #[weak]
                        cancel_transfer_button,
                        move |model_item| {
                            use rqs_lib::State;
                            let channel_message = model_item.channel_message();
                            if let Some(state) = channel_message.0.state {
                                match state {
                                    State::Initial => {}
                                    State::ReceivedConnectionRequest => {}
                                    State::SentUkeyServerInit => {}
                                    State::SentUkeyClientInit => {}
                                    State::SentUkeyClientFinish => {}
                                    State::SentPairedKeyEncryption => {}
                                    State::ReceivedUkeyClientFinish => {}
                                    State::SentConnectionResponse => {}
                                    State::SentPairedKeyResult => {}
                                    State::SentIntroduction => {}
                                    State::ReceivedPairedKeyResult => {}
                                    State::WaitingForUserConsent => {}
                                    State::ReceivingFiles => {
                                        accept_button.set_visible(false);
                                        decline_button.set_visible(false);
                                        cancel_transfer_button.set_visible(true);

                                        let receiving_text = {
                                            let channel_message = model_item.channel_message();
                                            if let Some(files) = channel_message.get_filenames() {
                                                formatx!(
                                                    ngettext(
                                                        "Receiving {} file...",
                                                        "Receiving {} files...",
                                                        files.len() as u32
                                                    ),
                                                    files.len()
                                                )
                                                .unwrap_or_default()
                                            } else {
                                                formatx!(
                                                    gettext("Receiving text <i>\"{}\"</i>"),
                                                    channel_message
                                                        .get_text_data()
                                                        .unwrap()
                                                        .description
                                                )
                                                .unwrap_or_default()
                                            }
                                        };
                                        caption_label.set_label(&receiving_text);
                                    }
                                    State::SendingFiles => {}
                                    State::Disconnected => {
                                        // FIXME: If ReceivingFiles is not received within 5~10 seconds of an Accept,
                                        // reject request and show this error, it's usually because the sender
                                        // disconnected from the network
                                        button_box.set_visible(false);
                                        result_label.set_visible(true);
                                        result_label
                                            .set_label(&gettext("Unexpected disconnection"));
                                        result_label.add_css_class("error");
                                    }
                                    State::Rejected => {
                                        button_box.set_visible(false);
                                        result_label.set_visible(true);
                                        result_label.set_label(&gettext("Rejected"));
                                        result_label.add_css_class("error");
                                    }
                                    State::Cancelled => {
                                        button_box.set_visible(false);
                                        result_label.set_visible(true);
                                        result_label.set_label(&gettext("Cancelled"));
                                        result_label.add_css_class("error");
                                    }
                                    State::Finished => {
                                        let finished_text = {
                                            let channel_message = model_item.channel_message();
                                            if let Some(files) = channel_message.get_filenames() {
                                                formatx!(
                                                    ngettext(
                                                        "Received {} file",
                                                        "Received {} files",
                                                        files.len() as u32
                                                    ),
                                                    files.len()
                                                )
                                                .unwrap_or_default()
                                            } else {
                                                formatx!(
                                                    gettext("Received text <i>\"{}\"</i>"),
                                                    channel_message
                                                        .get_text_data()
                                                        .unwrap()
                                                        .description
                                                )
                                                .unwrap_or_default()
                                            }
                                        };
                                        button_box.set_visible(false);
                                        caption_label.set_label(&finished_text);
                                        result_label.set_visible(true);
                                        result_label.set_label(&gettext("Finished"));
                                        result_label.add_css_class("success");
                                    }
                                };
                            }
                        }
                    ));
                }
                TransferKind::Send => {
                    let send_button = gtk::Button::builder()
                        .hexpand(true)
                        .can_shrink(false)
                        .label(gettext("Send"))
                        .css_classes(["pill", "suggested-action"])
                        .build();
                    button_box.append(&send_button);
                    button_box.append(&cancel_transfer_button);

                    fn send_files_cb(
                        id: String,
                        imp: &imp::QuickShareApplicationWindow,
                        model_item: &FileTransferObject,
                        file_sender: &std::sync::Arc<
                            tokio::sync::Mutex<
                                Option<tokio::sync::mpsc::Sender<rqs_lib::SendInfo>>,
                            >,
                        >,
                    ) {
                        let endpoint_info = model_item.endpoint_info();
                        let files_to_send = imp
                            .selected_files_to_send
                            .as_ref()
                            .borrow()
                            .clone()
                            .iter()
                            .filter_map(|it| it.to_str())
                            .map(|it| it.to_owned())
                            .collect::<Vec<_>>();

                        tokio_runtime().spawn(clone!(
                            #[strong]
                            id,
                            #[weak]
                            file_sender,
                            async move {
                                file_sender
                                    .lock()
                                    .await
                                    .as_mut()
                                    .unwrap()
                                    .send(rqs_lib::SendInfo {
                                        id: id.clone(),
                                        name: endpoint_info
                                            .name
                                            .clone()
                                            .unwrap_or(gettext("Unknown device")),
                                        addr: format!(
                                            "{}:{}",
                                            endpoint_info.ip.clone().unwrap_or_default(),
                                            endpoint_info.port.clone().unwrap_or_default()
                                        ),
                                        ob: rqs_lib::OutboundPayload::Files(files_to_send),
                                    })
                                    .await
                                    .unwrap();
                            }
                        ));
                    }

                    let file_sender = &imp.file_sender;
                    send_button.connect_clicked(clone!(
                        #[weak]
                        imp,
                        #[weak]
                        file_sender,
                        #[weak]
                        model_item,
                        #[strong]
                        id,
                        move |_| {
                            send_files_cb(id.clone(), &imp, &model_item, &file_sender);
                        }
                    ));

                    model_item.connect_endpoint_info_notify(clone!(
                        #[weak]
                        send_button,
                        move |model_item| {
                            if model_item.endpoint_info().present.is_none() {
                                send_button.set_sensitive(false);
                            } else {
                                send_button.set_sensitive(true);
                            }
                        }
                    ));

                    let retry_label = gettext("Retry");
                    model_item.connect_channel_message_notify(clone!(move |model_item| {
                        use rqs_lib::State;
                        let channel_message = model_item.channel_message();
                        if let Some(ref state) = channel_message.0.state {
                            match state {
                                State::Initial => {}
                                State::ReceivedConnectionRequest => {}
                                State::SentUkeyServerInit => {}
                                State::SentPairedKeyEncryption => {}
                                State::ReceivedUkeyClientFinish => {}
                                State::SentConnectionResponse => {}
                                State::SentPairedKeyResult => {}
                                State::ReceivedPairedKeyResult => {}
                                State::WaitingForUserConsent => {}
                                State::ReceivingFiles => {}
                                State::SentUkeyClientInit
                                | State::SentUkeyClientFinish
                                | State::SentIntroduction
                                | State::SendingFiles => {
                                    send_button.set_visible(false);
                                    cancel_transfer_button.set_visible(true);

                                    let receiving_text = {
                                        let file_count = channel_message
                                            .meta
                                            .as_ref()
                                            .unwrap()
                                            .files
                                            .as_ref()
                                            .unwrap()
                                            .len();
                                        formatx!(
                                            ngettext(
                                                "Sending {} file...",
                                                "Sending {} files...",
                                                file_count as u32
                                            ),
                                            file_count
                                        )
                                        .unwrap_or_default()
                                    };
                                    caption_label.set_label(&receiving_text);
                                }
                                State::Disconnected => {
                                    // FIXME: Wait for 5~10 seconds after a send and timeout
                                    // if did not receive SendingFiles within that timeframe
                                    // This is how google does it in their client
                                    cancel_transfer_button.set_visible(false);
                                    send_button.set_visible(true);
                                    send_button.set_label(&retry_label);

                                    result_label.set_visible(true);
                                    result_label.set_label(&gettext("Unexpected disconnection"));
                                    result_label.add_css_class("error");
                                }
                                State::Rejected => {
                                    cancel_transfer_button.set_visible(false);
                                    send_button.set_visible(true);
                                    send_button.set_label(&retry_label);

                                    result_label.set_visible(true);
                                    result_label.set_label(&gettext("Rejected"));
                                    result_label.add_css_class("error");
                                }
                                State::Cancelled => {
                                    cancel_transfer_button.set_visible(false);
                                    send_button.set_visible(true);
                                    send_button.set_label(&retry_label);

                                    result_label.set_visible(true);
                                    result_label.set_label(&gettext("Cancelled"));
                                    result_label.add_css_class("error");
                                }
                                State::Finished => {
                                    let finished_text = {
                                        let file_count = channel_message
                                            .meta
                                            .as_ref()
                                            .unwrap()
                                            .files
                                            .as_ref()
                                            .unwrap()
                                            .len();
                                        formatx!(
                                            ngettext(
                                                "Sent {} file",
                                                "Sent {} files",
                                                file_count as u32
                                            ),
                                            file_count
                                        )
                                        .unwrap_or_default()
                                    };

                                    button_box.set_visible(false);
                                    caption_label.set_label(&finished_text);
                                    result_label.set_visible(true);
                                    result_label.set_label(&gettext("Finished"));
                                    result_label.add_css_class("success");
                                }
                            };
                        }
                    }));
                }
            };

            adw::Bin::builder().child(&root_card_box).build()
        }

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

            // FIXME: Allow setting a const port number in app preferences and, download_path
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
                *imp.file_sender.lock().await = Some(file_sender);
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
                                tx.send(channel_message).await.unwrap();

                                // FIXME: Send desktop notification aswell
                                // send_request_notification(name, channel_msg.id.clone());
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
                        let channel_message = rx.recv().await.unwrap();

                        tracing::debug!(?channel_message, "Received on UI thread");

                        let id = &channel_message.id;

                        use rqs_lib::State;
                        match channel_message
                            .state
                            .clone()
                            .unwrap_or(rqs_lib::State::Initial)
                        {
                            State::Initial => {}
                            State::ReceivedConnectionRequest => {}
                            State::SentUkeyServerInit => {}
                            State::SentPairedKeyEncryption => {}
                            State::ReceivedUkeyClientFinish => {}
                            State::SentConnectionResponse => {}
                            State::SentPairedKeyResult => {}
                            State::ReceivedPairedKeyResult => {}
                            State::WaitingForUserConsent => {
                                // Receive file transfer requests
                                {
                                    // let name = file_transfer::ChannelMessage::get_device_name(
                                    //     &channel_message,
                                    // );
                                    // tracing::info!(
                                    //     ?channel_message,
                                    //     "{name} wants to start a transfer"
                                    // );

                                    let mut active_file_requests =
                                        imp.active_file_requests.lock().await;
                                    if let Some(file_transfer) = active_file_requests.get(id) {
                                        // Update file request state
                                        file_transfer.set_channel_message(
                                            file_transfer::ChannelMessage(channel_message),
                                        );
                                    } else {
                                        // Add new file request
                                        let obj = FileTransferObject::new(TransferKind::Receive);
                                        let id = id.clone();
                                        obj.set_channel_message(file_transfer::ChannelMessage(
                                            channel_message,
                                        ));
                                        imp.receive_file_transfer_model.insert(0, &obj);
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
                            State::SentUkeyClientInit
                            | State::SentUkeyClientFinish
                            | State::SentIntroduction
                            | State::Disconnected
                            | State::Rejected
                            | State::Cancelled
                            | State::Finished
                            | State::SendingFiles
                            | State::ReceivingFiles => {
                                match channel_message.rtype {
                                    Some(rqs_lib::channel::TransferType::Inbound) => {
                                        // Receive
                                        let active_file_requests =
                                            imp.active_file_requests.lock().await;
                                        if let Some(model_item) = active_file_requests.get(id) {
                                            model_item.set_channel_message(
                                                file_transfer::ChannelMessage(channel_message),
                                            );
                                        }
                                    }
                                    Some(rqs_lib::channel::TransferType::Outbound) => {
                                        // Send
                                        let active_discovered_endpoints =
                                            imp.active_discovered_endpoints.lock().await;

                                        if let Some(model_item) =
                                            active_discovered_endpoints.get(id)
                                        {
                                            model_item.set_channel_message(
                                                file_transfer::ChannelMessage(channel_message),
                                            );
                                        }
                                    }
                                    _ => {}
                                };
                            }
                        };
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
                async move {
                    let mdns_discovery_broadcast_tx = mdns_discovery_broadcast_tx
                        .lock()
                        .await
                        .as_ref()
                        .unwrap()
                        .clone();
                    let mut mdns_discovery_rx = mdns_discovery_broadcast_tx.subscribe();

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
                                if endpoint_info.present.is_none()
                                    && file_transfer.channel_message().state.is_none()
                                {
                                    // Endpoint disconnected, remove endpoint
                                    tracing::info!(
                                        ?endpoint_info,
                                        "Removing disconnected endpoint"
                                    );
                                    if let Some(pos) =
                                        imp.send_file_transfer_model.find(file_transfer)
                                    {
                                        imp.send_file_transfer_model.remove(pos);
                                    }
                                    active_discovered_endpoints.remove(&endpoint_info.id);
                                } else {
                                    // Update endpoint
                                    tracing::info!(?endpoint_info, "Updated endpoint info");
                                    file_transfer.set_endpoint_info(file_transfer::EndpointInfo(
                                        endpoint_info,
                                    ));
                                }
                            } else {
                                // Set new endpoint
                                tracing::info!(?endpoint_info, "Connected endpoint");
                                let obj = FileTransferObject::new(TransferKind::Send);
                                let id = endpoint_info.id.clone();
                                obj.set_endpoint_info(file_transfer::EndpointInfo(endpoint_info));
                                imp.send_file_transfer_model.insert(0, &obj);
                                active_discovered_endpoints.insert(id, obj);
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
                                // FIXME: Update visibility in UI?
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
                            // FIXME: Get visibility via a channel
                            // and temporarily make device visible?

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
