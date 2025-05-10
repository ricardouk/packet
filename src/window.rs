use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::glib::clone;
use gtk::{gdk, gio, glib};

use crate::application::QuickShareApplication;
use crate::config::{APP_ID, PROFILE};
use crate::objects::data_transfer::{self, DataTransferObject, TransferKind};
use crate::{tokio_runtime, widgets};

#[derive(Debug)]
pub enum LoopingTaskHandle {
    Tokio(tokio::task::JoinHandle<()>),
    Glib(glib::JoinHandle<()>),
}

mod imp {
    use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

    use tokio::sync::Mutex;

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

        // ---
        #[template_child]
        pub main_nav_view: TemplateChild<adw::NavigationView>,

        #[template_child]
        pub main_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub main_nav_content: TemplateChild<gtk::ScrolledWindow>,

        #[template_child]
        pub manage_files_nav_content: TemplateChild<gtk::Box>,
        #[template_child]
        pub manage_files_count_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub manage_files_add_files_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub manage_files_listbox: TemplateChild<gtk::ListBox>,
        #[default(gio::ListStore::new::<gio::File>())]
        pub manage_files_model: gio::ListStore,

        // ---
        #[template_child]
        pub receive_view_stack_page: TemplateChild<adw::ViewStackPage>,
        #[template_child]
        pub quick_settings_button: TemplateChild<gtk::Button>,

        #[template_child]
        pub bottom_bar_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub bottom_bar_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub bottom_bar_caption: TemplateChild<gtk::Label>,

        #[template_child]
        pub transfer_settings_dialog: TemplateChild<adw::Dialog>,

        #[template_child]
        pub receive_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub receive_file_transfer_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub send_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub send_drop_files_bin: TemplateChild<adw::Bin>,
        #[template_child]
        pub main_add_files_button: TemplateChild<gtk::Button>,
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
        pub device_name_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub device_visibility_switch: TemplateChild<adw::SwitchRow>,
        // #[template_child]
        // pub receive_idle_status_page: TemplateChild<adw::StatusPage>,
        pub rqs: Arc<Mutex<Option<rqs_lib::RQS>>>,
        pub file_sender: Arc<Mutex<Option<tokio::sync::mpsc::Sender<rqs_lib::SendInfo>>>>,
        pub ble_receiver: Arc<Mutex<Option<tokio::sync::broadcast::Receiver<()>>>>,
        pub mdns_discovery_broadcast_tx:
            Arc<Mutex<Option<tokio::sync::broadcast::Sender<rqs_lib::EndpointInfo>>>>,

        pub selected_files_to_send: Rc<RefCell<Vec<PathBuf>>>,

        #[default(gio::ListStore::new::<DataTransferObject>())]
        pub receive_file_transfer_model: gio::ListStore,
        #[default(gio::ListStore::new::<DataTransferObject>())]
        pub send_file_transfer_model: gio::ListStore,
        pub active_discovered_endpoints: Arc<Mutex<HashMap<String, DataTransferObject>>>,
        pub active_file_requests: Arc<Mutex<HashMap<String, DataTransferObject>>>,

        pub rqs_looping_async_tasks: RefCell<Vec<LoopingTaskHandle>>,
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
            obj.load_app_state();
            obj.setup_gactions();
            obj.setup_ui();
            // FIXME:! put it back
            // obj.setup_rqs_service();
        }
    }

    impl WidgetImpl for QuickShareApplicationWindow {}
    impl WindowImpl for QuickShareApplicationWindow {
        // Save window state on delete event
        fn close_request(&self) -> glib::Propagation {
            if let Err(err) = self.obj().save_window_size() {
                tracing::warn!("Failed to save window state, {}", &err);
            }
            if let Err(err) = self.obj().save_app_state() {
                tracing::warn!("Failed to save app state, {}", &err);
            }

            let (tx, rx) = async_channel::bounded(1);
            tokio_runtime().spawn(clone!(
                #[weak(rename_to = rqs)]
                self.rqs,
                async move {
                    {
                        let mut rqs_guard = rqs.lock().await;
                        if let Some(rqs) = rqs_guard.as_mut() {
                            rqs.stop().await;
                            tracing::info!("Stopped RQS service");
                        }
                    }

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

    fn save_app_state(&self) -> Result<(), glib::BoolError> {
        let imp = self.imp();

        imp.settings
            .set_string("device-name", imp.device_name_entry.text().as_str())?;

        Ok(())
    }

    fn load_app_state(&self) {}

    fn setup_gactions(&self) {
        // let toggle_visibility = gio::ActionEntry::builder("toggle-visibility")
        //     .state(false.to_variant())
        //     .activate(|win: &Self, action, _| {
        //         let action_state: bool = action.state().unwrap().get().unwrap();
        //         let new_state = !action_state;
        //         action.set_state(&new_state.to_variant());
        //         // callback here
        //     })
        //     .build();

        // self.add_action_entries([toggle_visibility]);
    }

    fn get_device_name_state(&self) -> glib::GString {
        self.imp().settings.string("device-name")
    }

    fn set_device_name_state(&self, s: &str) -> Result<(), glib::BoolError> {
        self.imp().settings.set_string("device-name", s)
    }

    fn setup_ui(&self) {
        let imp = self.imp();

        // FIXME:! remove test code
        imp.root_stack.set_visible_child_name("main_page");

        // FIXME: Add filters, don't accept directories
        // Only files!
        let files_drop_target = gtk::DropTarget::builder()
            .name("add-files-drop-target")
            .actions(gdk::DragAction::COPY)
            .formats(&gdk::ContentFormats::for_type(gdk::FileList::static_type()))
            .build();
        // FIXME: add the drop zone to the next select files page as well
        imp.main_nav_content
            .get()
            .add_controller(files_drop_target.clone());

        fn filter_added_files(model: &gio::ListStore, mut files: Vec<gio::File>) -> Vec<gio::File> {
            files
                .into_iter()
                .filter(|file| {
                    file.query_file_type(
                        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                        gio::Cancellable::NONE,
                    ) == gio::FileType::Regular
                })
                .filter(|file| {
                    for existing_file in model.iter::<gio::File>().filter_map(|it| it.ok()) {
                        if existing_file.parse_name() == file.parse_name() {
                            return false;
                        }
                    }

                    true
                })
                .collect::<Vec<_>>()
        }

        files_drop_target.connect_drop(clone!(
            #[weak]
            imp,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                imp.manage_files_model.remove_all();
                if let Ok(file_list) = value.get::<gdk::FileList>() {
                    select_files_to_send_cb(&imp, file_list.files());
                }

                false
            }
        ));

        let manage_files_add_drop_target = gtk::DropTarget::builder()
            .name("manage-files-add-drop-target")
            .actions(gdk::DragAction::COPY)
            .formats(&gdk::ContentFormats::for_type(gdk::FileList::static_type()))
            .build();
        imp.manage_files_nav_content
            .get()
            .add_controller(manage_files_add_drop_target.clone());
        manage_files_add_drop_target.connect_drop(clone!(
            #[weak]
            imp,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                if let Ok(file_list) = value.get::<gdk::FileList>() {
                    select_files_to_send_cb(
                        &imp,
                        filter_added_files(&imp.manage_files_model, file_list.files()),
                    );
                }

                false
            }
        ));

        // imp.main_nav_view.get().push_by_tag("transfer_history_page");

        let device_name = &self.get_device_name_state();
        let device_name_entry = imp.device_name_entry.get();
        {
            if device_name.is_empty() {
                let device_name = whoami::devicename();
                device_name_entry.set_text(&device_name);
                // Can't use bind, since that's not the behaviour we want
                // We need to keep a state of entry widget before apply so
                // that we can restore the name to what's actually being used
                self.set_device_name_state(&device_name).unwrap();
            } else {
                device_name_entry.set_text(device_name);
            }
        }
        device_name_entry.connect_apply(clone!(
            #[weak(rename_to = this)]
            self,
            move |entry| {
                entry.set_editable(false);
                this.set_device_name(entry.text().as_str());
                entry.set_editable(true);
            }
        ));

        imp.manage_files_listbox.bind_model(
            Some(&imp.manage_files_model),
            clone!(
                #[weak]
                imp,
                #[upgrade_or]
                adw::Bin::new().into(),
                move |model| {
                    let model_item = model.downcast_ref::<gio::File>().unwrap();
                    widgets::create_file_card(&imp.obj(), &imp.manage_files_model, model_item)
                        .into()
                }
            ),
        );

        fn select_files_to_send_cb(imp: &imp::QuickShareApplicationWindow, files: Vec<gio::File>) {
            if files.len() == 0 {
                // FIXME: Show toast about not being able to access files
            } else {
                // FIXME: don't accept files that are already in the list
                // No duplicates!
                let file_count = files.len() + imp.manage_files_model.n_items() as usize;
                imp.manage_files_count_label.set_label(
                    &formatx!(
                        ngettext("{} File", "{} Files", file_count as u32),
                        file_count
                    )
                    .unwrap(),
                );

                if let Some(tag) = imp.main_nav_view.visible_page_tag() {
                    if &tag != "manage_files_nav_page" {
                        imp.main_nav_view.push_by_tag("manage_files_nav_page");
                    }
                }

                for file in &files {
                    imp.manage_files_model.append(file);
                }

                // imp.send_stack
                //     .get()
                //     .set_visible_child_name("send_nearby_devices_page");

                // FIXME:!
                // let title = formatx!(
                //     &ngettext(
                //         "{} file is ready to send",
                //         "{} files are ready to send",
                //         files.len() as u32,
                //     ),
                //     files.len()
                // )
                // .unwrap_or_default();

                // imp.selected_files_card_title.get().set_label(&title);
                // imp.selected_files_to_send.as_ref().borrow_mut().clear();

                // for file in &files {
                //     tracing::info!(file = ?file.path(), "Selected file");
                //     if let Some(path) = file.path() {
                //         imp.selected_files_to_send.as_ref().borrow_mut().push(path);
                //     }
                // }

                // imp.selected_files_card_caption.get().set_label(
                //     &imp.selected_files_to_send
                //         .as_ref()
                //         .borrow()
                //         .iter()
                //         .map(|it| it.file_name().and_then(|it| Some(it.to_string_lossy())))
                //         .flatten()
                //         .collect::<Vec<_>>()
                //         .join(", "),
                // );

                // FIXME:!
                // imp.obj().start_mdns_discovery();
            }
        }

        fn select_files_via_dialog(imp: &imp::QuickShareApplicationWindow) {
            gtk::FileDialog::new().open_multiple(
                imp.obj()
                    .root()
                    .and_downcast_ref::<adw::ApplicationWindow>(),
                None::<&gio::Cancellable>,
                clone!(
                    #[weak]
                    imp,
                    move |files| {
                        if let Ok(files) = files {
                            let mut files_vec = Vec::with_capacity(files.n_items() as usize);
                            for i in 0..files.n_items() {
                                let file = files.item(i).unwrap().downcast::<gio::File>().unwrap();
                                files_vec.push(file);
                            }

                            select_files_to_send_cb(
                                &imp,
                                filter_added_files(&imp.manage_files_model, files_vec),
                            );
                        };
                    }
                ),
            );
        }

        imp.manage_files_add_files_button.connect_clicked(clone!(
            #[weak]
            imp,
            move |_| {
                select_files_via_dialog(&imp);
            }
        ));
        imp.main_add_files_button.connect_clicked(clone!(
            #[weak]
            imp,
            move |_| {
                imp.manage_files_model.remove_all();
                select_files_via_dialog(&imp);
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

                    imp.obj().stop_mdns_discovery();

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
                    let model_item = obj.downcast_ref::<DataTransferObject>().unwrap();
                    widgets::create_data_transfer_card(
                        &imp.obj(),
                        &imp.send_file_transfer_model,
                        model_item,
                    )
                    .into()
                }
            ),
        );
        // FIXME:!
        // send_file_transfer_model.connect_items_changed(clone!(
        //     #[weak]
        //     imp,
        //     move |model, _, _, _| {
        //         let loading_nearby_devices_box = imp.loading_nearby_devices_box.get();
        //         if model.n_items() == 0 {
        //             loading_nearby_devices_box.set_visible(true);
        //         } else {
        //             loading_nearby_devices_box.set_visible(false);
        //         }
        //     }
        // ));

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
                    let model_item = obj.downcast_ref::<DataTransferObject>().unwrap();
                    widgets::create_data_transfer_card(
                        &imp.obj(),
                        &imp.receive_file_transfer_model,
                        model_item,
                    )
                    .into()
                }
            ),
        );

        // FIXME:!
        // receive_file_transfer_model.connect_items_changed(clone!(
        //     #[weak]
        //     imp,
        //     move |model, _, _, _| {
        //         if model.n_items() == 0 {
        //             imp.receive_stack
        //                 .set_visible_child_name("receive_idle_status_page");
        //         } else {
        //             imp.receive_stack
        //                 .set_visible_child_name("receive_request_page");
        //         }
        //     }
        // ));

        self.setup_bottom_bar();
    }

    fn setup_bottom_bar(&self) {
        let imp = self.imp();

        fn visibility_toggle_ui_update(
            obj: &adw::SwitchRow,
            imp: &imp::QuickShareApplicationWindow,
        ) {
            tracing::info!("HELLO");
            if obj.is_active() {
                imp.bottom_bar_title.set_label(&gettext("Ready to receive"));
                imp.bottom_bar_image.set_icon_name(Some("sonar-symbolic"));
                imp.bottom_bar_caption.set_label(
                    &formatx!(
                        gettext("Visible as {:?}"),
                        imp.obj().get_device_name_state().as_str()
                    )
                    .unwrap(),
                );
            } else {
                imp.bottom_bar_title.set_label(&gettext("Invisible"));
                imp.bottom_bar_image
                    .set_icon_name(Some("background-app-ghost-symbolic"));
                imp.bottom_bar_caption
                    .set_label(&gettext("No new devices can share with you"));
            };
        }

        imp.quick_settings_button.connect_clicked(clone!(
            #[weak]
            imp,
            move |_| {
                imp.transfer_settings_dialog
                    .present(Some(imp.obj().as_ref()));
            }
        ));

        let device_visibility_switch = imp.device_visibility_switch.get();
        visibility_toggle_ui_update(&device_visibility_switch, &imp);
        device_visibility_switch.connect_active_notify(clone!(
            #[weak]
            imp,
            move |obj| {
                visibility_toggle_ui_update(&obj, &imp);

                let _visibility = if obj.is_active() {
                    rqs_lib::Visibility::Visible
                } else {
                    rqs_lib::Visibility::Invisible
                };

                // FIXME:!
                // imp.rqs
                //     .blocking_lock()
                //     .as_mut()
                //     .unwrap()
                //     .change_visibility(visibility);
            }
        ));
    }

    fn start_mdns_discovery(&self) {
        let imp = self.imp();

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

    fn stop_mdns_discovery(&self) {
        let imp = self.imp();

        tokio_runtime().spawn(clone!(
            #[weak(rename_to = rqs)]
            imp.rqs,
            async move {
                rqs.lock().await.as_mut().unwrap().stop_discovery();
            }
        ));
    }

    fn is_no_file_being_send(&self) -> bool {
        let imp = self.imp();

        for model_item in imp
            .send_file_transfer_model
            .iter::<DataTransferObject>()
            .filter_map(|it| it.ok())
        {
            use rqs_lib::State;
            match model_item
                .channel_message()
                .state
                .as_ref()
                .unwrap_or(&rqs_lib::State::Initial)
            {
                State::Initial
                | State::Disconnected
                | State::Rejected
                | State::Cancelled
                | State::Finished => {}
                _ => {
                    return false;
                }
            }
        }

        true
    }

    fn set_device_name(&self, name: &str) {
        let imp = self.imp();

        // Since transfers from this device to other devices will be affected,
        // we won't proceed if they exist
        if self.is_no_file_being_send() {
            // FIXME: Show a progress dialog conveying service restart?

            imp.rqs
                .blocking_lock()
                .as_mut()
                .expect("State must be set")
                .set_device_name(name.to_string());

            let (tx, rx) = async_channel::bounded(1);
            tokio_runtime().spawn(clone!(
                #[weak(rename_to = rqs)]
                imp.rqs,
                async move {
                    let (file_sender, ble_receiver) = {
                        let mut guard = rqs.lock().await;
                        let rqs = guard.as_mut().unwrap();

                        rqs.stop().await;
                        rqs.run().await.unwrap()
                    };

                    tx.send((file_sender, ble_receiver)).await.unwrap();
                }
            ));
            glib::spawn_future_local(clone!(
                #[weak]
                imp,
                async move {
                    let (file_sender, ble_receiver) = rx.recv().await.unwrap();

                    *imp.file_sender.lock().await = Some(file_sender);
                    *imp.ble_receiver.lock().await = Some(ble_receiver);

                    // FIXME: instead of this, turn on mdns_discovery if it was on before
                    imp.selected_files_card_cancel_button.emit_clicked();

                    tracing::debug!("RQS service has been reset");

                    // FIXME: Show a toast for device name change success?

                    imp.device_visibility_switch.set_active(true);
                }
            ));
        } else {
            imp.device_name_entry.set_show_apply_button(false);
            imp.device_name_entry
                .set_text(&self.get_device_name_state());
            imp.device_name_entry.set_show_apply_button(true);

            tracing::debug!("Active transfers found, can't rename device name");

            // FIXME: Show a dialog/toast conveying that name change is not allowed while
            // files are being send to other devices
        }
    }

    fn setup_rqs_service(&self) {
        let imp = self.imp();

        let (tx, rx) = async_channel::bounded(1);

        let device_name = self.get_device_name_state();
        tokio_runtime().spawn(async move {
            let download_path = directories::UserDirs::new()
                .unwrap()
                .download_dir()
                .unwrap()
                .to_path_buf();

            tracing::info!(?download_path, "Starting RQS service");

            // FIXME: Allow setting a const port number in app preferences and, download_path
            let mut rqs = rqs_lib::RQS::new(
                rqs_lib::Visibility::Visible,
                None,
                Some(download_path),
                Some(device_name.to_string()),
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
                *imp.file_sender.lock().await = Some(file_sender);
                *imp.ble_receiver.lock().await = Some(ble_receiver);

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
            let handle = tokio_runtime().spawn(clone!(
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
            imp.rqs_looping_async_tasks
                .borrow_mut()
                .push(LoopingTaskHandle::Tokio(handle));

            let handle = glib::spawn_future_local(clone!(
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
                                            data_transfer::ChannelMessage(channel_message),
                                        );
                                    } else {
                                        // Add new file request
                                        let obj = DataTransferObject::new(TransferKind::Receive);
                                        let id = id.clone();
                                        obj.set_channel_message(data_transfer::ChannelMessage(
                                            channel_message,
                                        ));
                                        imp.receive_file_transfer_model.insert(0, &obj);
                                        active_file_requests.insert(id, obj);

                                        imp.receive_view_stack_page.set_badge_number(
                                            imp.receive_view_stack_page.badge_number() + 1,
                                        );
                                        imp.receive_view_stack_page.set_needs_attention(true);
                                    }
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
                                                data_transfer::ChannelMessage(channel_message),
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
                                                data_transfer::ChannelMessage(channel_message),
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
            imp.rqs_looping_async_tasks
                .borrow_mut()
                .push(LoopingTaskHandle::Glib(handle));

            // MDNS discovery receiver
            // Discover the devices to send file transfer requests to
            // The Sender used in RQS::discovery()
            let (tx, rx) = async_channel::bounded(1);
            let handle = tokio_runtime().spawn(clone!(
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
            imp.rqs_looping_async_tasks
                .borrow_mut()
                .push(LoopingTaskHandle::Tokio(handle));

            let handle = glib::spawn_future_local(clone!(
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
                                    file_transfer.set_endpoint_info(data_transfer::EndpointInfo(
                                        endpoint_info,
                                    ));
                                }
                            } else {
                                // Set new endpoint
                                tracing::info!(?endpoint_info, "Connected endpoint");
                                let obj = DataTransferObject::new(TransferKind::Send);
                                let id = endpoint_info.id.clone();
                                obj.set_endpoint_info(data_transfer::EndpointInfo(endpoint_info));
                                imp.send_file_transfer_model.insert(0, &obj);
                                active_discovered_endpoints.insert(id, obj);
                            }
                        }
                    }
                }
            ));
            imp.rqs_looping_async_tasks
                .borrow_mut()
                .push(LoopingTaskHandle::Glib(handle));

            let handle = tokio_runtime().spawn(clone!(
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
            imp.rqs_looping_async_tasks
                .borrow_mut()
                .push(LoopingTaskHandle::Tokio(handle));

            // FIXME: Since renaming device name will restart the service,
            // we need to reset the ble_receiver here in the loop as well.
            // Ideal solution seem to be to keep a handle on this async task
            // and close it when we set device name and respawn it.
            // tokio_runtime().spawn(clone!(
            //     #[weak(rename_to = ble_receiver)]
            //     imp.ble_receiver,
            //     async move {
            //         let mut ble_receiver =
            //             ble_receiver.lock().await.as_ref().unwrap().resubscribe();
            //         // let mut last_sent = std::time::Instant::now() - std::time::Duration::from_secs(120);
            //         loop {
            //             match ble_receiver.recv().await {
            //                 Ok(_) => {
            //                     // let is_visible = device_visibility_switch.is_active();
            //                     // FIXME: Get visibility via a channel
            //                     // and temporarily make device visible?

            //                     tracing::debug!("Received BLE")
            //                 }
            //                 Err(err) => {
            //                     tracing::error!(%err,"Error receiving BLE");
            //                 }
            //             }
            //         }
            //     }
            // ));
        }
    }
}
