use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::{
    gio,
    glib::{self, clone},
};
use rqs_lib::hdl::TextPayloadType;

use crate::{
    objects::{self, TransferState},
    window::{LoopingTaskHandle, QuickShareApplicationWindow},
};

mod imp {

    use std::{cell::RefCell, rc::Rc};

    use glib::Properties;

    use crate::{objects, utils::DataTransferEta};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, Properties)]
    #[template(resource = "/io/github/nozwock/QuickShare/ui/share-request-dialog.ui")]
    #[properties(wrapper_type = super::ShareRequestDialog)]
    pub struct ShareRequestDialog {
        #[template_child]
        pub toolbar_view: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        pub copy_text_button: TemplateChild<gtk::Button>,

        #[template_child]
        pub root_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub heading_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub caption_label: TemplateChild<gtk::Label>,

        #[template_child]
        pub consent_box: TemplateChild<gtk::Box>,

        #[template_child]
        pub progress_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub eta_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub progress_bar: TemplateChild<gtk::ProgressBar>,

        #[template_child]
        pub text_view_frame: TemplateChild<gtk::Frame>,
        #[template_child]
        pub text_view: TemplateChild<gtk::TextView>,

        #[template_child]
        pub open_uri_button: TemplateChild<gtk::Button>,

        #[property(get, set)]
        pub win: RefCell<Option<QuickShareApplicationWindow>>,

        #[property(get, set)]
        pub event: RefCell<objects::ChannelMessage>,
        pub eta: Rc<RefCell<DataTransferEta>>,

        #[property(get, set)]
        pub transfer_state: RefCell<objects::TransferState>,

        pub event_rx: RefCell<Option<async_channel::Receiver<objects::ChannelMessage>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ShareRequestDialog {
        const NAME: &'static str = "PacketShareRequestDialog";
        type Type = super::ShareRequestDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_instance_callbacks();
        }

        // You must call `Widget`'s `init_template()` within `instance_init()`.
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ShareRequestDialog {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            // obj.setup_ui();
        }
    }
    impl WidgetImpl for ShareRequestDialog {}
    impl AdwDialogImpl for ShareRequestDialog {}
}

glib::wrapper! {
    pub struct ShareRequestDialog(ObjectSubclass<imp::ShareRequestDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ShareRequestDialog {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}

#[gtk::template_callbacks]
impl ShareRequestDialog {
    pub fn new(
        win: QuickShareApplicationWindow,
        event: objects::ChannelMessage,
        event_rx: async_channel::Receiver<objects::ChannelMessage>,
    ) -> Self {
        let obj: Self = glib::Object::builder().build();
        obj.set_event(&event);
        *obj.imp().event_rx.borrow_mut() = Some(event_rx);
        obj.imp()
            .eta
            .borrow_mut()
            .prepare_for_new_transfer(Some(event.meta.as_ref().unwrap().total_bytes as usize));
        obj.set_win(win);

        // Is this okay? feels weird
        obj.setup_ui();

        obj
    }

    #[template_callback]
    fn handle_consent_accept(&self, button: &gtk::Button) {
        let win = self.win().unwrap();

        button.set_sensitive(false);
        win.imp()
            .rqs
            .blocking_lock()
            .as_mut()
            .unwrap()
            .message_sender
            .send(rqs_lib::channel::ChannelMessage {
                id: self.event().id.to_string(),
                action: Some(rqs_lib::channel::ChannelAction::AcceptTransfer),
                ..Default::default()
            })
            .unwrap();
    }
    #[template_callback]
    fn handle_consent_decline(&self, _: &gtk::Button) {
        let win = self.win().unwrap();

        self.close();
        win.imp()
            .rqs
            .blocking_lock()
            .as_mut()
            .unwrap()
            .message_sender
            .send(rqs_lib::channel::ChannelMessage {
                id: self.event().id.to_string(),
                action: Some(rqs_lib::channel::ChannelAction::RejectTransfer),
                ..Default::default()
            })
            .unwrap();
    }
    #[template_callback]
    fn handle_transfer_cancel(&self, button: &gtk::Button) {
        let win = self.win().unwrap();

        button.set_sensitive(false);
        win.imp()
            .rqs
            .blocking_lock()
            .as_mut()
            .unwrap()
            .message_sender
            .send(rqs_lib::channel::ChannelMessage {
                id: self.event().id.to_string(),
                action: Some(rqs_lib::channel::ChannelAction::CancelTransfer),
                ..Default::default()
            })
            .unwrap();
    }
    #[template_callback]
    fn handle_uri_open(&self, _: &gtk::Button) {
        let imp = self.imp();

        let url = imp.text_view.buffer().text(
            &imp.text_view.buffer().start_iter(),
            &imp.text_view.buffer().end_iter(),
            false,
        );

        gtk::UriLauncher::new(&url).launch(
            imp.obj()
                .root()
                .and_downcast_ref::<adw::ApplicationWindow>(),
            None::<gio::Cancellable>.as_ref(),
            |_err| {},
        );
    }
    #[template_callback]
    fn handle_copy_text(&self, _: &gtk::Button) {
        let clipboard = self.clipboard();

        let imp = self.imp();

        let text = imp.text_view.buffer().text(
            &imp.text_view.buffer().start_iter(),
            &imp.text_view.buffer().end_iter(),
            false,
        );
        clipboard.set_text(&text);
    }

    pub fn setup_ui(&self) {
        let imp = self.imp();

        let win = self.win().unwrap();

        let rqs = &win.imp().rqs;
        // close-attempt doesn't seem to trigger at all
        self.connect_closed(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            rqs,
            move |_| {
                if this.transfer_state() == TransferState::AwaitingConsentOrIdle {
                    tracing::debug!("SHOULD TRIGGER?");
                    rqs.blocking_lock()
                        .as_mut()
                        .unwrap()
                        .message_sender
                        .send(rqs_lib::channel::ChannelMessage {
                            id: this.event().id.to_string(),
                            action: Some(rqs_lib::channel::ChannelAction::RejectTransfer),
                            ..Default::default()
                        })
                        .unwrap();
                }
            }
        ));

        let msg = self.event();
        // Setting initial state for WaitingForUserContent
        {
            // Present
            self.present(Some(&win));

            imp.consent_box.set_visible(true);
            imp.caption_label.set_visible(true);

            imp.progress_box.set_visible(false);

            imp.heading_label.set_label(&gettext("Incoming Request"));

            let total_bytes = msg.meta.as_ref().unwrap().total_bytes;

            imp.eta
                .borrow_mut()
                .prepare_for_new_transfer(Some(total_bytes as usize));

            let caption = if let Some(files) = msg.get_filenames() {
                formatx!(
                    ngettext(
                        "{} wants to share {} file ({})",
                        "{} wants to share {} files ({})",
                        files.len() as u32
                    ),
                    msg.get_device_name(),
                    files.len(),
                    human_bytes::human_bytes(total_bytes as f64)
                )
                .unwrap_or_default()
            } else {
                formatx!(
                    gettext("{} wants to share <i>{}</i>"),
                    msg.get_device_name(),
                    msg.get_text_data().unwrap().description.replace("\n", "")
                )
                .unwrap_or_default()
            };

            imp.caption_label.set_label(&caption);
        }

        let init_id = msg.id.clone();
        let handle = glib::spawn_future_local(clone!(
            #[weak]
            imp,
            async move {
                use rqs_lib::State;
                let rx = &imp.event_rx;

                loop {
                    let msg = rx.borrow().as_ref().unwrap().recv().await;

                    if let Ok(msg) = &msg {
                        imp.obj().set_event(msg);
                    }

                    if let Some((state, msg)) = msg
                        .ok()
                        .and_then(|it| Some((it.state.clone().unwrap_or(State::Initial), it)))
                    {
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
                                imp.obj().set_can_close(false);
                                imp.obj().set_transfer_state(TransferState::OngoingTransfer);
                                imp.caption_label.set_visible(false);
                                imp.progress_box.set_visible(true);
                                imp.consent_box.set_visible(false);

                                imp.heading_label.set_label(&gettext("Receiving"));

                                let eta_text = {
                                    if let Some(meta) = &msg.meta {
                                        imp.eta.borrow_mut().step_with(meta.ack_bytes as usize);

                                        if meta.total_bytes > 0 {
                                            imp.progress_bar.set_fraction(
                                                meta.ack_bytes as f64 / meta.total_bytes as f64,
                                            );
                                        }
                                    }

                                    formatx!(
                                        gettext("About {} left"),
                                        imp.eta.borrow().get_estimate_string()
                                    )
                                    .unwrap()
                                };
                                imp.eta_label.set_label(&eta_text);
                            }
                            State::SendingFiles => {}
                            State::Disconnected => {
                                imp.obj().set_can_close(true);
                                imp.obj().set_transfer_state(TransferState::Failed);
                                if msg.id == init_id {
                                    // FIXME: If ReceivingFiles is not received within 5~10 seconds of an Accept,
                                    // reject request and show this error, it's usually because the sender
                                    // disconnected from the network
                                    imp.progress_box.set_visible(false);
                                    imp.caption_label.set_visible(true);
                                    imp.consent_box.set_visible(false);
                                    imp.header_bar.set_show_end_title_buttons(true);

                                    imp.caption_label
                                        .set_label(&gettext("Unexpected disconnection"));
                                    break;
                                }
                            }
                            State::Rejected => {
                                imp.obj().set_can_close(true);
                                imp.obj().set_transfer_state(TransferState::Failed);
                                break;
                            }
                            State::Cancelled => {
                                imp.obj().set_can_close(true);
                                imp.obj().set_transfer_state(TransferState::Failed);
                                imp.header_bar.set_show_end_title_buttons(true);

                                imp.progress_box.set_visible(false);
                                imp.caption_label.set_visible(true);
                                imp.consent_box.set_visible(false);
                                imp.header_bar.set_show_end_title_buttons(true);

                                imp.caption_label.set_label(&gettext("Failed"));

                                break;
                            }
                            State::Finished => {
                                imp.obj().set_can_close(true);
                                imp.obj().set_transfer_state(TransferState::Done);
                                imp.progress_box.set_visible(false);
                                imp.caption_label.set_visible(true);
                                imp.consent_box.set_visible(false);
                                imp.header_bar.set_show_end_title_buttons(true);

                                imp.heading_label.set_visible(false);
                                imp.obj().set_title(&gettext("Done"));
                                imp.toolbar_view.set_extend_content_to_top_edge(false);
                                imp.root_box.set_margin_top(0);

                                {
                                    if let Some(files) = msg.get_filenames() {
                                        let text = formatx!(
                                            ngettext(
                                                "Received {} file",
                                                "Received {} files",
                                                files.len() as u32
                                            ),
                                            files.len()
                                        )
                                        .unwrap_or_default();

                                        imp.caption_label.set_label(&text);
                                    } else {
                                        imp.copy_text_button.set_visible(true);
                                        // FIXME: Can't handle WiFi shares yet
                                        // TextPayloadInfo not exposed by the library
                                        let text_type = msg.get_text_data().unwrap().kind.unwrap();

                                        if text_type.clone() as u32 == TextPayloadType::Url as u32 {
                                            imp.open_uri_button.set_visible(true);
                                        } else {
                                            imp.open_uri_button.set_visible(false);
                                        }

                                        // FIXME: add a "save as" button to save text view content
                                        // as a file

                                        let _text = msg.get_text_data().unwrap().text;
                                        let text = if text_type.clone() as u32
                                            == TextPayloadType::Text as u32
                                        {
                                            let text = _text.trim();
                                            &text[1..text.len() - 1] // Remove quotes put in there by the lib :(
                                        } else {
                                            &_text
                                        };

                                        imp.caption_label.set_label(&format!(
                                            "Received {}",
                                            format!("{:?}", text_type)
                                        ));
                                        imp.text_view_frame.set_visible(true);
                                        imp.text_view.set_buffer(Some(
                                            &gtk::TextBuffer::builder().text(text).build(),
                                        ));

                                        if text_type.clone() as u32 == TextPayloadType::Wifi as u32
                                        {
                                            imp.caption_label.set_label(
                                                &formatx!(
                                                    &gettext("{}, not implemented yet :("),
                                                    &imp.caption_label.label()
                                                )
                                                .unwrap(),
                                            );
                                        }
                                    }
                                };

                                break;
                            }
                        }
                    }
                }
            }
        ));

        win.imp()
            .looping_async_tasks
            .borrow_mut()
            .push(LoopingTaskHandle::Glib(handle));
    }
}
