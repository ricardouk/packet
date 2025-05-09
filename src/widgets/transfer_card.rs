use std::cell::RefCell;

use crate::{
    objects::{
        self,
        file_transfer::{FileTransferObject, TransferKind},
    },
    tokio_runtime,
    window::QuickShareApplicationWindow,
};

use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib, glib::clone};
use rqs_lib::channel::ChannelMessage;

pub fn create_file_transfer_card(
    win: &QuickShareApplicationWindow,
    model: &gio::ListStore,
    model_item: &FileTransferObject,
) -> adw::Bin {
    let imp = win.imp();
    let (caption, title) = match model_item.transfer_kind() {
        TransferKind::Receive => {
            let device_name =
                objects::ChannelMessage::get_device_name(&model_item.channel_message().0);

            let caption = if let Some(files) = model_item.channel_message().get_filenames() {
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

    let top_box = gtk::Box::builder().build();
    main_box.append(&top_box);

    // FIXME: UI for request transfer pin code
    // `object-select-symbolic` for success status icon
    let device_icon_image = adw::Avatar::builder()
        .text(&title)
        .show_initials(true)
        .size(48)
        .build();
    top_box.append(&device_icon_image);

    let label_box = gtk::Box::builder()
        .margin_start(18)
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    top_box.append(&label_box);

    top_box.append(&adw::Bin::builder().hexpand(true).build());
    let clear_card_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .icon_name("cross-large-symbolic")
        .css_classes(["flat", "circular"])
        .tooltip_text(&gettext("Dismiss"))
        .visible(false)
        .build();
    top_box.append(&clear_card_button);

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

    // FIXME: When the transfer is cancelled, the progress bar still keeps going
    // and the card doesn't move to the cancelled state immediately.
    // Disregard what's happening in the backend and update the UI anyways, the incomplete
    // transferred files will be deleted anyways
    let progress_bar = gtk::ProgressBar::builder().visible(false).build();
    main_box.append(&progress_bar);

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

    fn set_progress_bar_fraction(
        progress_bar: &gtk::ProgressBar,
        channel_message: &ChannelMessage,
    ) {
        if let Some(meta) = &channel_message.meta {
            if meta.total_bytes > 0 {
                progress_bar.set_fraction(meta.ack_bytes as f64 / meta.total_bytes as f64);
            }
        }
    }

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

            clear_card_button.connect_clicked(clone!(
                #[weak]
                imp,
                #[weak]
                model,
                #[weak]
                model_item,
                move |_| {
                    if let Some(pos) = model.find(&model_item) {
                        model.remove(pos);
                    }
                    imp.active_file_requests
                        .blocking_lock()
                        .remove(&model_item.channel_message().id);
                }
            ));

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

            fn lower_notification_count(win: &QuickShareApplicationWindow, state: &RefCell<bool>) {
                let imp = win.imp();
                if !*state.borrow() {
                    let badge_count = imp.receive_view_stack_page.badge_number();
                    if badge_count > 0 {
                        imp.receive_view_stack_page
                            .set_badge_number(badge_count - 1);

                        if badge_count - 1 == 0 {
                            imp.receive_view_stack_page.set_needs_attention(false);
                        }
                    }

                    *state.borrow_mut() = true;
                }
            }

            // FIXME: Add new model properties like `title`, `caption`, `card_state`
            // and so the ui can be updated by setting this properties outside of the UI
            // code section, while we listen to property changes here
            // And, this way the UI can be easily reproduced as well based on the model state
            // unlike here. This is important for a transfer history page since that page
            // will be built out of a list based on these model states
            // Or,
            // if possible via ListStore, just copy the widget instead of going model -> widget

            let is_badge_removed_state = RefCell::new(false);
            model_item.connect_channel_message_notify(clone!(
                #[weak]
                imp,
                #[weak]
                cancel_transfer_button,
                move |model_item| {
                    use rqs_lib::State;

                    let channel_message = model_item.channel_message();
                    if let Some(state) = &channel_message.0.state {
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

                                progress_bar.set_visible(true);
                                set_progress_bar_fraction(&progress_bar, &channel_message);

                                lower_notification_count(&imp.obj(), &is_badge_removed_state);

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
                                            channel_message.get_text_data().unwrap().description
                                        )
                                        .unwrap_or_default()
                                    }
                                };
                                caption_label.set_label(&receiving_text);
                            }
                            State::SendingFiles => {}
                            State::Disconnected => {
                                lower_notification_count(&imp.obj(), &is_badge_removed_state);
                                // FIXME: If ReceivingFiles is not received within 5~10 seconds of an Accept,
                                // reject request and show this error, it's usually because the sender
                                // disconnected from the network
                                progress_bar.set_visible(false);
                                clear_card_button.set_visible(true);
                                button_box.set_visible(false);
                                result_label.set_visible(true);
                                result_label.set_label(&gettext("Unexpected disconnection"));
                                result_label.add_css_class("error");
                            }
                            State::Rejected => {
                                lower_notification_count(&imp.obj(), &is_badge_removed_state);
                                progress_bar.set_visible(false);
                                clear_card_button.set_visible(true);
                                button_box.set_visible(false);
                                result_label.set_visible(true);
                                result_label.set_label(&gettext("Rejected"));
                                result_label.add_css_class("error");
                            }
                            State::Cancelled => {
                                lower_notification_count(&imp.obj(), &is_badge_removed_state);
                                progress_bar.set_visible(false);
                                clear_card_button.set_visible(true);
                                button_box.set_visible(false);
                                result_label.set_visible(true);
                                result_label.set_label(&gettext("Cancelled"));
                                result_label.add_css_class("error");
                            }
                            State::Finished => {
                                progress_bar.set_visible(false);
                                clear_card_button.set_visible(true);
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
                                            channel_message.get_text_data().unwrap().description
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

            clear_card_button.connect_clicked(clone!(
                #[weak]
                imp,
                #[weak]
                model,
                #[weak]
                model_item,
                move |_| {
                    if let Some(pos) = model.find(&model_item) {
                        model.remove(pos);
                    }
                    imp.active_discovered_endpoints
                        .blocking_lock()
                        .remove(&model_item.channel_message().id);
                }
            ));

            fn send_files_cb(
                id: String,
                win: &QuickShareApplicationWindow,
                model_item: &FileTransferObject,
                file_sender: &std::sync::Arc<
                    tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<rqs_lib::SendInfo>>>,
                >,
            ) {
                let imp = win.imp();
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
                    send_files_cb(id.clone(), &imp.obj(), &model_item, &file_sender);
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
            model_item.connect_channel_message_notify(move |model_item| {
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

                            progress_bar.set_visible(true);
                            set_progress_bar_fraction(&progress_bar, &channel_message);

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
                            progress_bar.set_visible(false);
                            cancel_transfer_button.set_visible(false);
                            send_button.set_visible(true);
                            send_button.set_label(&retry_label);

                            result_label.set_visible(true);
                            result_label.set_label(&gettext("Unexpected disconnection"));
                            result_label.add_css_class("error");
                        }
                        State::Rejected => {
                            progress_bar.set_visible(false);
                            cancel_transfer_button.set_visible(false);
                            send_button.set_visible(true);
                            send_button.set_label(&retry_label);

                            result_label.set_visible(true);
                            result_label.set_label(&gettext("Rejected"));
                            result_label.add_css_class("error");
                        }
                        State::Cancelled => {
                            progress_bar.set_visible(false);
                            cancel_transfer_button.set_visible(false);
                            send_button.set_visible(true);
                            send_button.set_label(&retry_label);

                            result_label.set_visible(true);
                            result_label.set_label(&gettext("Cancelled"));
                            result_label.add_css_class("error");
                        }
                        State::Finished => {
                            progress_bar.set_visible(false);
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
                                    ngettext("Sent {} file", "Sent {} files", file_count as u32),
                                    file_count
                                )
                                .unwrap_or_default()
                            };

                            clear_card_button.set_visible(true);
                            button_box.set_visible(false);
                            caption_label.set_label(&finished_text);
                            result_label.set_visible(true);
                            result_label.set_label(&gettext("Finished"));
                            result_label.add_css_class("success");
                        }
                    };
                }
            });
        }
    };

    adw::Bin::builder().child(&root_card_box).build()
}
