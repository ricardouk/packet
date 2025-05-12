use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::{
    gio,
    glib::{self, clone},
};
use rqs_lib::hdl::TextPayloadType;

use crate::{objects, utils, window::QuickShareApplicationWindow};

#[derive(Debug, Clone, Default)]
pub struct ReceiveRequestState {
    pub msg: objects::ChannelMessage,
    pub eta: utils::DataTransferEta,
}

pub fn create_receive_request_dialog(
    win: &QuickShareApplicationWindow,
    receive_state: &Rc<RefCell<ReceiveRequestState>>,
    event_receiver: async_channel::Receiver<objects::ChannelMessage>,
) {
    let imp = win.imp();

    let dialog = adw::Dialog::new();
    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .extend_content_to_top_edge(true)
        .extend_content_to_bottom_edge(true)
        .build();
    dialog.set_child(Some(&toolbar_view));

    let header_bar = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .build();
    toolbar_view.add_top_bar(&header_bar);

    let clamp = adw::Clamp::builder()
        .maximum_size(550)
        .hexpand(true)
        .vexpand(true)
        .build();
    toolbar_view.set_content(Some(&clamp));

    let root_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .spacing(18)
        .build();
    clamp.set_child(Some(&root_box));

    let title_label = gtk::Label::builder()
        .label(&gettext("Incoming Request"))
        .css_classes(["title-1"])
        .build();
    let caption_label = gtk::Label::builder().use_markup(true).wrap(true).build();
    root_box.append(&title_label);
    root_box.append(&caption_label);

    let progress_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .visible(false)
        .build();
    root_box.append(&progress_box);

    let eta_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .build();
    let progress_bar = gtk::ProgressBar::new();
    // FIXME: confirmation for cancelling
    let cancel_transfer_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .margin_top(12)
        .label(&gettext("Cancel"))
        .css_classes(["pill"])
        .build();
    progress_box.append(&eta_label);
    progress_box.append(&progress_bar);
    progress_box.append(&cancel_transfer_button);

    let consent_box = gtk::Box::builder().hexpand(true).spacing(12).build();
    root_box.append(&consent_box);
    let consent_decline_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .hexpand(true)
        .height_request(50)
        .label(&gettext("Decline"))
        .build();
    let consent_accept_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .hexpand(true)
        .height_request(50)
        .label(&gettext("Accept"))
        .css_classes(["suggested-action"])
        .build();
    consent_box.append(&consent_decline_button);
    consent_box.append(&consent_accept_button);

    let text_view = gtk::TextView::builder()
        .top_margin(12)
        .bottom_margin(12)
        .left_margin(12)
        .right_margin(12)
        .editable(false)
        .monospace(true)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::Word)
        .build();

    let text_view_frame = gtk::Frame::builder()
        .child(&gtk::ScrolledWindow::builder().child(&text_view).build())
        .visible(false)
        .build();
    root_box.append(&text_view_frame);

    let copy_text_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .hexpand(true)
        .height_request(50)
        .icon_name("edit-copy-symbolic")
        .tooltip_text(&gettext("Copy to clipboard"))
        .css_classes(["circular", "flat"])
        .visible(false)
        .build();
    let open_uri_button = gtk::Button::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .height_request(50)
        .label(&gettext("Open"))
        .css_classes(["pill", "suggested-action"])
        .visible(false)
        .build();
    header_bar.pack_end(&copy_text_button);
    root_box.append(&open_uri_button);

    let clipboard = win.clipboard();

    copy_text_button.connect_clicked(clone!(
        #[weak]
        text_view,
        #[strong]
        clipboard,
        move |_| {
            let text = text_view.buffer().text(
                &text_view.buffer().start_iter(),
                &text_view.buffer().end_iter(),
                false,
            );
            clipboard.set_text(&text);
        }
    ));

    open_uri_button.connect_clicked(clone!(
        #[weak]
        win,
        #[weak]
        text_view,
        move |_| {
            let url = text_view.buffer().text(
                &text_view.buffer().start_iter(),
                &text_view.buffer().end_iter(),
                false,
            );

            gtk::UriLauncher::new(&url).launch(
                win.root().and_downcast_ref::<adw::ApplicationWindow>(),
                None::<gio::Cancellable>.as_ref(),
                |_err| {},
            );
        }
    ));

    consent_decline_button.connect_clicked(clone!(
        #[weak(rename_to = rqs)]
        imp.rqs,
        #[weak]
        receive_state,
        #[weak]
        dialog,
        move |_| {
            dialog.close();
            rqs.blocking_lock()
                .as_mut()
                .unwrap()
                .message_sender
                .send(rqs_lib::channel::ChannelMessage {
                    id: receive_state.borrow().msg.id.to_string(),
                    action: Some(rqs_lib::channel::ChannelAction::RejectTransfer),
                    ..Default::default()
                })
                .unwrap();
        }
    ));
    consent_accept_button.connect_clicked(clone!(
        #[weak(rename_to = rqs)]
        imp.rqs,
        #[weak]
        receive_state,
        move |button| {
            button.set_sensitive(false);
            rqs.blocking_lock()
                .as_mut()
                .unwrap()
                .message_sender
                .send(rqs_lib::channel::ChannelMessage {
                    id: receive_state.borrow().msg.id.to_string(),
                    action: Some(rqs_lib::channel::ChannelAction::AcceptTransfer),
                    ..Default::default()
                })
                .unwrap();
        }
    ));
    cancel_transfer_button.connect_clicked(clone!(
        #[weak(rename_to = rqs)]
        imp.rqs,
        #[weak]
        receive_state,
        move |button| {
            button.set_sensitive(false);
            rqs.blocking_lock()
                .as_mut()
                .unwrap()
                .message_sender
                .send(rqs_lib::channel::ChannelMessage {
                    id: receive_state.borrow().msg.id.to_string(),
                    action: Some(rqs_lib::channel::ChannelAction::CancelTransfer),
                    ..Default::default()
                })
                .unwrap();
        }
    ));
    dialog.connect_close_attempt(clone!(
        #[weak(rename_to = rqs)]
        imp.rqs,
        #[strong]
        receive_state,
        move |obj| {
            use rqs_lib::State;
            let action = match receive_state.borrow().msg.state {
                Some(State::WaitingForUserConsent) => {
                    obj.set_can_close(true);
                    Some(rqs_lib::channel::ChannelAction::RejectTransfer)
                }
                Some(State::ReceivingFiles) => {
                    obj.set_can_close(false);
                    None
                }
                _ => {
                    obj.set_can_close(true);
                    None
                }
            };

            if let Some(action) = action {
                rqs.blocking_lock()
                    .as_mut()
                    .unwrap()
                    .message_sender
                    .send(rqs_lib::channel::ChannelMessage {
                        id: receive_state.borrow().msg.id.to_string(),
                        action: Some(action),
                        ..Default::default()
                    })
                    .unwrap();
            }
        }
    ));

    let msg = receive_state.borrow().msg.clone();
    // Setting initial state for WaitingForUserContent
    {
        dialog.present(imp.obj().root().as_ref());

        consent_accept_button.set_sensitive(true);

        consent_box.set_visible(true);
        caption_label.set_visible(true);

        progress_box.set_visible(false);

        title_label.set_label(&gettext("Incoming Request"));

        let total_bytes = msg.meta.as_ref().unwrap().total_bytes;

        receive_state
            .borrow_mut()
            .eta
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

        caption_label.set_label(&caption);
    }

    let init_id = msg.id.clone();
    glib::spawn_future_local(clone!(
        #[strong]
        receive_state,
        async move {
            use rqs_lib::State;
            let rx = event_receiver;

            loop {
                let msg = rx.recv().await;

                if let Ok(msg) = &msg {
                    receive_state.borrow_mut().msg = msg.clone();
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
                            caption_label.set_visible(false);
                            progress_box.set_visible(true);
                            consent_box.set_visible(false);

                            title_label.set_label(&gettext("Receiving"));

                            let eta_text = {
                                if let Some(meta) = &msg.meta {
                                    receive_state
                                        .borrow_mut()
                                        .eta
                                        .step_with(meta.ack_bytes as usize);

                                    if meta.total_bytes > 0 {
                                        progress_bar.set_fraction(
                                            meta.ack_bytes as f64 / meta.total_bytes as f64,
                                        );
                                    }
                                }

                                formatx!(
                                    gettext("About {} left"),
                                    receive_state.borrow().eta.get_estimate_string()
                                )
                                .unwrap()
                            };
                            eta_label.set_label(&eta_text);
                        }
                        State::SendingFiles => {}
                        State::Disconnected => {
                            // FIXME: If ReceivingFiles is not received within 5~10 seconds of an Accept,
                            // reject request and show this error, it's usually because the sender
                            // disconnected from the network
                            progress_box.set_visible(false);
                            caption_label.set_visible(true);
                            consent_box.set_visible(false);
                            header_bar.set_show_end_title_buttons(true);

                            caption_label.set_label(&gettext("Unexpected disconnection"));
                            break;
                        }
                        State::Rejected => {
                            break;
                        }
                        State::Cancelled => {
                            header_bar.set_show_end_title_buttons(true);

                            progress_box.set_visible(false);
                            caption_label.set_visible(true);
                            consent_box.set_visible(false);
                            header_bar.set_show_end_title_buttons(true);

                            caption_label.set_label(&gettext("Failed"));

                            break;
                        }
                        State::Finished => {
                            progress_box.set_visible(false);
                            caption_label.set_visible(true);
                            consent_box.set_visible(false);
                            header_bar.set_show_end_title_buttons(true);
                            copy_text_button.set_visible(true);

                            title_label.set_visible(false);
                            dialog.set_title(&gettext("Done"));
                            toolbar_view.set_extend_content_to_top_edge(false);
                            root_box.set_margin_top(0);

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

                                    caption_label.set_label(&text);
                                } else {
                                    // FIXME: Can't handle WiFi shares yet
                                    // TextPayloadInfo not exposed by the library
                                    let text_type = msg.get_text_data().unwrap().kind.unwrap();

                                    if text_type.clone() as u32 == TextPayloadType::Url as u32 {
                                        open_uri_button.set_visible(true);
                                    } else {
                                        open_uri_button.set_visible(false);
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

                                    caption_label.set_label(&format!(
                                        "Received {}",
                                        format!("{:?}", text_type)
                                    ));
                                    text_view_frame.set_visible(true);
                                    text_view.set_buffer(Some(
                                        &gtk::TextBuffer::builder().text(text).build(),
                                    ));

                                    if text_type.clone() as u32 == TextPayloadType::Wifi as u32 {
                                        caption_label.set_label(
                                            &formatx!(
                                                &gettext("{}, not implemented yet :("),
                                                &caption_label.label()
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
}
