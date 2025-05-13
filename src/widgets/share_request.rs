use std::{cell::Cell, rc::Rc};

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
    objects::{self},
    window::QuickShareApplicationWindow,
};

pub fn display_text_type(value: &TextPayloadType) -> String {
    match value {
        TextPayloadType::Url => gettext("Link"),
        TextPayloadType::Text => gettext("Text"),
        TextPayloadType::Wifi => gettext("Wi-Fi"),
    }
}

// Rewriting receive UI for the 4rd time ;(
// Using a chain of AlertDialog this time
pub fn present_share_request_ui(
    win: &QuickShareApplicationWindow,
    receive_state: &objects::ShareRequestState,
) {
    let init_id = receive_state.event().id.clone();
    let win = win.clone();

    // Progress dialog
    let progress_dialog = adw::AlertDialog::builder()
        .heading(&gettext("Receiving"))
        .width_request(200)
        .build();
    progress_dialog.add_responses(&[("cancel", &gettext("Cancel"))]);
    progress_dialog.set_default_response(None);
    progress_dialog.connect_response(
        None,
        clone!(
            #[weak]
            win,
            #[weak]
            receive_state,
            move |dialog, response_id| {
                match response_id {
                    "cancel" => {
                        // FIXME: show a toast notifying that the transfer was cancelled?
                        dialog.set_response_enabled("cancel", false);
                        win.imp()
                            .rqs
                            .blocking_lock()
                            .as_mut()
                            .unwrap()
                            .message_sender
                            .send(rqs_lib::channel::ChannelMessage {
                                id: receive_state.event().id.to_string(),
                                action: Some(rqs_lib::channel::ChannelAction::CancelTransfer),
                                ..Default::default()
                            })
                            .unwrap();
                    }
                    _ => {}
                }
            }
        ),
    );
    progress_dialog.set_can_close(false);

    let progress_stack = gtk::Stack::new();

    let progress_files_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(24)
        .margin_end(24)
        .spacing(12)
        .build();
    progress_stack.add_named(&progress_files_box, Some("progress_files"));

    let device_name = receive_state.event().get_device_name();
    let device_name_box = create_device_name_box(&device_name);
    device_name_box.set_margin_bottom(4);
    progress_files_box.append(&device_name_box);

    let progress_bar = gtk::ProgressBar::new();
    progress_files_box.append(&progress_bar);
    let eta_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .wrap(true)
        .css_classes(["dimmed"])
        .build();
    progress_files_box.append(&eta_label);

    let progress_text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(24)
        .margin_end(24)
        .spacing(12)
        .build();

    let device_name_box = create_device_name_box(&device_name);
    device_name_box.set_margin_bottom(4);
    progress_text_box.append(&device_name_box);
    progress_text_box.append(&adw::Spinner::new());
    progress_stack.add_named(&progress_text_box, Some("progress_text"));

    progress_dialog.set_extra_child(Some(&progress_stack));

    fn create_device_name_box(device_name: &str) -> gtk::Box {
        let device_name_box = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .spacing(8)
            .build();
        let avatar = adw::Avatar::builder()
            .text(device_name)
            .show_initials(true)
            .size(32)
            .build();
        device_name_box.append(&avatar);
        let device_label = gtk::Label::builder()
            .label(device_name)
            .halign(gtk::Align::Center)
            .css_classes(["title-4"])
            .build();
        device_name_box.append(&device_label);

        device_name_box
    }

    let consent_dialog = adw::AlertDialog::builder()
        .heading(&gettext("Incoming Transfer"))
        .width_request(200)
        .build();

    let is_request_accepted = Rc::new(Cell::new(false));
    receive_state.connect_event_notify(move |receive_state| {
        use rqs_lib::State;

        let msg = receive_state.event();

        match msg.state.clone().unwrap_or(State::Initial) {
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
            State::WaitingForUserConsent => {
                consent_dialog.add_responses(&[
                    ("decline", &gettext("Decline")),
                    ("accept", &gettext("Accept")),
                ]);
                consent_dialog
                    .set_response_appearance("accept", adw::ResponseAppearance::Suggested);

                consent_dialog.set_default_response(Some("decline"));
                consent_dialog.set_close_response("decline");

                let info_box = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .halign(gtk::Align::Center)
                    .spacing(8)
                    .build();
                consent_dialog.set_extra_child(Some(&info_box));

                let device_name = msg.get_device_name();

                let device_name_box = create_device_name_box(&device_name);
                info_box.append(&device_name_box);

                let total_bytes = msg.meta.as_ref().unwrap().total_bytes;
                let transfer_size = human_bytes::human_bytes(total_bytes as f64);

                if let Some(files) = msg.get_filenames() {
                    let file_count = files.len();

                    let files_label = gtk::Label::builder()
                        .label(
                            formatx!(
                                ngettext("{} file ({})", "{} files ({})", file_count as u32,),
                                file_count,
                                transfer_size
                            )
                            .unwrap(),
                        )
                        .halign(gtk::Align::Center)
                        .css_classes(["dimmed", "heading"])
                        .build();
                    info_box.append(&files_label);
                } else {
                    let text_data = msg.get_text_data().unwrap();
                    let text_info_label = gtk::Label::builder()
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .max_width_chars(36)
                        .label(
                            formatx!(
                                gettext("Preview ({})"),
                                text_data
                                    .description
                                    .trim()
                                    .trim_matches('"')
                                    .trim()
                                    .lines()
                                    .next()
                                    .unwrap_or_default()
                                    .trim()
                            )
                            .unwrap_or_default(),
                        )
                        .halign(gtk::Align::Center)
                        .css_classes(["dimmed"])
                        .build();
                    info_box.append(&text_info_label);
                }

                let pincode_label = gtk::Label::builder()
                    .label(
                        formatx!(
                            gettext("Code: {}"),
                            msg.meta
                                .as_ref()
                                .unwrap()
                                .pin_code
                                .clone()
                                .unwrap_or_default()
                        )
                        .unwrap(),
                    )
                    .halign(gtk::Align::Center)
                    .css_classes(["dimmed", "monospace"])
                    .build();
                info_box.append(&pincode_label);

                consent_dialog.connect_response(
                    None,
                    clone!(
                        #[weak]
                        win,
                        #[weak]
                        receive_state,
                        #[weak]
                        progress_dialog,
                        #[weak]
                        is_request_accepted,
                        move |_, response_id| {
                            match response_id {
                                "accept" => {
                                    win.imp()
                                        .rqs
                                        .blocking_lock()
                                        .as_mut()
                                        .unwrap()
                                        .message_sender
                                        .send(rqs_lib::channel::ChannelMessage {
                                            id: receive_state.event().id.to_string(),
                                            action: Some(
                                                rqs_lib::channel::ChannelAction::AcceptTransfer,
                                            ),
                                            ..Default::default()
                                        })
                                        .unwrap();

                                    // Spawn progress dialog
                                    progress_dialog.present(Some(&win));
                                    is_request_accepted.replace(true);
                                }
                                "decline" => {
                                    win.imp()
                                        .rqs
                                        .blocking_lock()
                                        .as_mut()
                                        .unwrap()
                                        .message_sender
                                        .send(rqs_lib::channel::ChannelMessage {
                                            id: receive_state.event().id.to_string(),
                                            action: Some(
                                                rqs_lib::channel::ChannelAction::RejectTransfer,
                                            ),
                                            ..Default::default()
                                        })
                                        .unwrap();
                                }
                                _ => {
                                    unreachable!()
                                }
                            };
                        }
                    ),
                );

                consent_dialog.present(Some(&win));

                // TODO: show a progress dialog for both but with a delay?

                // Create Progress bar dialog
                let total_bytes = msg.meta.as_ref().unwrap().total_bytes;
                receive_state
                    .imp()
                    .eta
                    .borrow_mut()
                    .prepare_for_new_transfer(Some(total_bytes as usize));
                if msg.get_text_data().is_some() {
                    progress_stack.set_visible_child_name("progress_text");
                }
            }
            State::ReceivingFiles => {
                if msg.get_text_data().is_none() {
                    let eta_text = {
                        if let Some(meta) = &msg.meta {
                            receive_state
                                .imp()
                                .eta
                                .borrow_mut()
                                .step_with(meta.ack_bytes as usize);

                            if meta.total_bytes > 0 {
                                progress_bar
                                    .set_fraction(meta.ack_bytes as f64 / meta.total_bytes as f64);
                            }
                        }

                        formatx!(
                            gettext("About {} left"),
                            receive_state.imp().eta.borrow().get_estimate_string()
                        )
                        .unwrap()
                    };
                    eta_label.set_label(&eta_text);
                }
            }
            State::SendingFiles => {}
            State::Disconnected => {
                if msg.id == init_id {
                    progress_dialog.set_can_close(true);
                    if is_request_accepted.get() {
                        progress_dialog.close();
                    } else {
                        consent_dialog.close();
                    }

                    win.imp().toast_overlay.add_toast(
                        adw::Toast::builder()
                            .title(gettext("Unexpected dissconnection"))
                            .priority(adw::ToastPriority::High)
                            .build(),
                    );

                    // FIXME: If ReceivingFiles is not received within 5~10 seconds of an Accept,
                    // reject request and show this error, it's usually because the sender
                    // disconnected from the network
                }
            }
            State::Rejected => {}
            State::Cancelled => {
                progress_dialog.set_can_close(true);
                if is_request_accepted.get() {
                    progress_dialog.close();
                } else {
                    consent_dialog.close();
                }

                win.imp().toast_overlay.add_toast(
                    adw::Toast::builder()
                        .title(gettext("Transfer cancelled by sender"))
                        .priority(adw::ToastPriority::High)
                        .build(),
                );
            }
            State::Finished => {
                progress_dialog.set_can_close(true);
                if is_request_accepted.get() {
                    progress_dialog.close();
                } else {
                    consent_dialog.close();
                }

                if let Some(text_data) = msg.get_text_data() {
                    let text_type = text_data.kind.unwrap();

                    let dialog = adw::Dialog::builder()
                        .content_width(400)
                        .content_height(200)
                        .title(display_text_type(&text_type))
                        .build();

                    let toolbar_view = adw::ToolbarView::builder()
                        .top_bar_style(adw::ToolbarStyle::Flat)
                        .build();
                    dialog.set_child(Some(&toolbar_view));

                    let header_bar = adw::HeaderBar::builder().build();
                    toolbar_view.add_top_bar(&header_bar);

                    let copy_text_button = gtk::Button::builder()
                        .valign(gtk::Align::Center)
                        .hexpand(true)
                        .icon_name("edit-copy-symbolic")
                        .tooltip_text(&gettext("Copy to clipboard"))
                        .css_classes(["circular", "flat"])
                        .build();
                    let save_text_button = gtk::Button::builder()
                        .visible(false)
                        .valign(gtk::Align::Center)
                        .hexpand(true)
                        .icon_name("document-save-symbolic")
                        .tooltip_text(&gettext("Save text as file"))
                        .css_classes(["circular", "flat"])
                        .build();
                    header_bar.pack_start(&copy_text_button);
                    header_bar.pack_start(&save_text_button);

                    let clamp = adw::Clamp::builder()
                        .maximum_size(550)
                        .hexpand(true)
                        .vexpand(true)
                        .build();
                    toolbar_view.set_content(Some(&clamp));

                    let root_box = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .hexpand(true)
                        .margin_top(6)
                        .margin_bottom(18)
                        .margin_start(18)
                        .margin_end(18)
                        .spacing(18)
                        .build();
                    clamp.set_child(Some(&root_box));

                    let caption_label = gtk::Label::builder()
                        .use_markup(true)
                        .wrap(true)
                        .visible(false)
                        .build();
                    root_box.append(&caption_label);

                    let text_view = gtk::TextView::builder()
                        .top_margin(12)
                        .bottom_margin(12)
                        .left_margin(12)
                        .right_margin(12)
                        .editable(false)
                        .cursor_visible(false)
                        .monospace(true)
                        .wrap_mode(gtk::WrapMode::Word)
                        .build();

                    let text_view_frame = gtk::Frame::builder()
                        .vexpand(true)
                        .child(
                            &gtk::ScrolledWindow::builder()
                                .vexpand(true)
                                .child(&text_view)
                                .build(),
                        )
                        .build();
                    root_box.append(&text_view_frame);

                    let open_uri_button = gtk::Button::builder()
                        .halign(gtk::Align::Center)
                        .valign(gtk::Align::Center)
                        .height_request(50)
                        .label(&gettext("Open"))
                        .css_classes(["pill", "suggested-action"])
                        .build();
                    root_box.append(&open_uri_button);
                    if text_type.clone() as u32 == TextPayloadType::Url as u32 {
                        open_uri_button.set_visible(true);
                    } else {
                        open_uri_button.set_visible(false);
                    }

                    save_text_button.connect_clicked(clone!(
                        #[weak]
                        win,
                        #[weak]
                        text_view,
                        move |_| {
                            let text = text_view.buffer().text(
                                &text_view.buffer().start_iter(),
                                &text_view.buffer().end_iter(),
                                false,
                            );

                            glib::spawn_future_local(async move {
                                let file = gtk::FileDialog::new()
                                    .save_text_file_future(Some(&win))
                                    .await
                                    .unwrap()
                                    .0
                                    .unwrap();

                                let text_bytes = text.into_bytes();
                                file.create_readwrite_future(
                                    gio::FileCreateFlags::REPLACE_DESTINATION,
                                    Default::default(),
                                )
                                .await
                                .unwrap()
                                .output_stream()
                                .write_all_future(text_bytes, Default::default())
                                .await
                                .unwrap();
                            });
                        }
                    ));

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

                    // FIXME: Can't handle WiFi shares yet
                    // TextPayloadInfo not exposed by the library
                    let text_type = msg.get_text_data().unwrap().kind.unwrap();

                    let _text = msg.get_text_data().unwrap().text;
                    let text = if text_type.clone() as u32 == TextPayloadType::Text as u32 {
                        save_text_button.set_visible(true);
                        let text = _text.trim();
                        &text[1..text.len() - 1] // Remove quotes put in there by the lib :(
                    } else {
                        &_text
                    };
                    text_view.set_buffer(Some(&gtk::TextBuffer::builder().text(text).build()));

                    if text_type.clone() as u32 == TextPayloadType::Wifi as u32 {
                        caption_label.set_visible(true);
                        caption_label.set_label(&gettext("Unimplemented"));
                    }

                    dialog.present(Some(&win));
                } else {
                    // Received Files
                    let file_count = msg.get_filenames().unwrap().len();
                    let toast = adw::Toast::builder()
                        .title(
                            &formatx!(
                                ngettext(
                                    "{} file received",
                                    "{} files received",
                                    file_count as u32
                                ),
                                file_count
                            )
                            .unwrap(),
                        )
                        .button_label(&gettext("Open"))
                        .action_name("win.received-files")
                        .priority(adw::ToastPriority::High)
                        .build();
                    win.imp().toast_overlay.add_toast(toast);
                }
            }
        }
    });
    receive_state.notify_event();
}
