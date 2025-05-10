use std::cell::RefCell;

use crate::{
    objects::{
        self,
        data_transfer::{DataTransferObject, TransferKind},
    },
    tokio_runtime,
    utils::DataTransferEta,
    window::QuickShareApplicationWindow,
};

use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib, glib::clone};
use rqs_lib::channel::ChannelMessage;

fn get_model_item_from_listbox<T>(
    model: &gio::ListStore,
    list_box: &gtk::ListBox,
    row: &gtk::ListBoxRow,
) -> Option<T>
where
    T: IsA<glib::Object>,
{
    let mut pos = 0;
    while let Some(x) = list_box.row_at_index(pos) {
        if x == *row {
            break;
        }
        pos = pos + 1;
    }

    model
        .item(pos as u32)
        .and_then(|it| it.downcast::<T>().ok())
}

fn get_listbox_row_from_model_item<T>(
    model: &gio::ListStore,
    list_box: &gtk::ListBox,
    model_item: &T,
) -> Option<gtk::ListBoxRow>
where
    T: IsA<glib::Object>,
{
    let mut pos = 0;
    while let Some(x) = model.item(pos) {
        if x.downcast_ref::<T>().unwrap() == model_item {
            break;
        }
        pos = pos + 1;
    }

    list_box.row_at_index(pos as i32)
}

pub fn handle_recipient_card_clicked(
    win: &QuickShareApplicationWindow,
    list_box: &gtk::ListBox,
    row: &gtk::ListBoxRow,
) {
    let imp = win.imp();

    let model_item =
        get_model_item_from_listbox::<DataTransferObject>(&imp.recipient_model, list_box, row)
            .unwrap();

    let endpoint_info = model_item.endpoint_info();

    let files_to_send = imp
        .manage_files_model
        .iter::<gio::File>()
        .filter_map(|it| it.ok())
        .filter_map(|it| it.path())
        .map(|it| it.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    tokio_runtime().spawn(clone!(
        #[weak(rename_to = file_sender)]
        imp.file_sender,
        async move {
            file_sender
                .lock()
                .await
                .as_mut()
                .unwrap()
                .send(rqs_lib::SendInfo {
                    id: endpoint_info.id.clone(),
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

pub fn create_recipient_card(
    win: &QuickShareApplicationWindow,
    _model: &gio::ListStore,
    model_item: &DataTransferObject,
) -> adw::Bin {
    // Send Only!
    assert_eq!(model_item.transfer_kind(), TransferKind::Send);

    let imp = win.imp();

    let title = model_item
        .endpoint_info()
        .name
        .clone()
        .unwrap_or(gettext("Unknown device").into());

    // `card` style will be applied with `boxed-list*` on ListBox
    // v/h-align would prevent the card from expanding when space is available
    let root_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .spacing(12)
        .build();
    let root_bin = adw::Bin::builder().child(&root_box).build();

    // FIXME: UI for request transfer pin code
    // `object-select-symbolic` for success status icon

    // FIXME: Use file icons based on mimetype
    // These are the icons that Files/nautilus uses
    // https://gitlab.gnome.org/GNOME/adwaita-icon-theme/-/tree/master/Adwaita/scalable?ref_type=heads
    let device_avatar = adw::Avatar::builder()
        .text(&title)
        .show_initials(true)
        .size(48)
        .build();
    root_box.append(&device_avatar);

    let right_box = gtk::Box::builder().build();
    root_box.append(&right_box);

    let main_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .spacing(6)
        .build();
    right_box.append(&main_box);

    let title_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .label(title)
        .css_classes(["title-4"])
        .build();
    let result_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["caption"])
        .build();
    main_box.append(&title_label);
    main_box.append(&result_label);

    let progress_bar = gtk::ProgressBar::builder().visible(false).build();
    main_box.append(&progress_bar);

    let total_size = imp
        .manage_files_model
        .iter::<gio::File>()
        .filter_map(|it| it.ok())
        .filter_map(|it| {
            it.query_info(
                gio::FILE_ATTRIBUTE_STANDARD_SIZE,
                gio::FileQueryInfoFlags::NONE,
                None::<&gio::Cancellable>,
            )
            .ok()
        })
        .map(|it| it.size() as usize)
        .fold(0, |acc, x| acc + x);

    tracing::debug!(total_size);

    let eta_estimator = RefCell::new(DataTransferEta::new(total_size));

    let eta_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["caption", "dim-label"])
        .build();
    main_box.append(&eta_label);

    let id = match model_item.transfer_kind() {
        TransferKind::Receive => model_item.channel_message().id.clone(),
        TransferKind::Send => model_item.endpoint_info().id.clone(),
    };

    root_box.append(&adw::Bin::builder().hexpand(true).build());
    let cancel_transfer_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .icon_name("cross-large-symbolic")
        .css_classes(["circular"])
        .tooltip_text(&gettext("Cancel"))
        .visible(false)
        .build();
    root_box.append(&cancel_transfer_button);
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

    fn set_row_activatable(
        model_item: &DataTransferObject,
        row: &gtk::ListBoxRow,
        activatable: bool,
    ) {
        if model_item.endpoint_info().present.is_none() {
            row.set_activatable(false);
        } else {
            row.set_activatable(activatable);
        }
    }
    model_item.connect_endpoint_info_notify(clone!(
        #[weak]
        imp,
        move |model_item| {
            if let Some(row) = get_listbox_row_from_model_item::<DataTransferObject>(
                &imp.recipient_model,
                &imp.recipient_listbox,
                model_item,
            ) {
                set_row_activatable(model_item, &row, true);
            };
        }
    ));

    model_item.connect_channel_message_notify(clone!(
        #[weak]
        imp,
        move |model_item| {
            use rqs_lib::State;

            let _endpoint_info = model_item.endpoint_info();
            let channel_message = model_item.channel_message();

            let clickable_row_to_send = get_listbox_row_from_model_item::<DataTransferObject>(
                &imp.recipient_model,
                &imp.recipient_listbox,
                model_item,
            )
            .unwrap();

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
                    | State::SentIntroduction => {
                        set_row_activatable(model_item, &clickable_row_to_send, false);
                        cancel_transfer_button.set_visible(true);
                        cancel_transfer_button.set_sensitive(true);

                        result_label.set_visible(true);
                        result_label.set_label(&gettext("Requested"));
                        result_label.set_css_classes(&["caption", "accent"]);

                        eta_estimator.borrow_mut().prepare_for_new_transfer(None);
                    }
                    State::SendingFiles => {
                        set_row_activatable(model_item, &clickable_row_to_send, false);
                        cancel_transfer_button.set_visible(true);
                        cancel_transfer_button.set_sensitive(true);
                        result_label.set_visible(false);
                        eta_label.set_visible(true);
                        let eta_text = {
                            if let Some(meta) = &channel_message.meta {
                                eta_estimator
                                    .borrow_mut()
                                    .step_with(meta.ack_bytes as usize);
                            }

                            formatx!(
                                gettext("About {} left"),
                                eta_estimator.borrow().get_estimate_string()
                            )
                            .unwrap()
                        };
                        eta_label.set_label(&eta_text);

                        progress_bar.set_visible(true);
                        set_progress_bar_fraction(&progress_bar, &channel_message);
                    }
                    State::Disconnected => {
                        // FIXME: Wait for 5~10 seconds after a send and timeout
                        // if did not receive SendingFiles within that timeframe
                        // This is how google does it in their client
                        set_row_activatable(model_item, &clickable_row_to_send, true);
                        progress_bar.set_visible(false);
                        cancel_transfer_button.set_visible(false);
                        eta_label.set_visible(false);

                        result_label.set_visible(true);
                        result_label.set_label(&gettext("Failed"));
                        result_label.set_css_classes(&["caption", "error"]);
                    }
                    State::Rejected => {
                        // FIXME: Outbound(Reject) is not handled on lib side
                        // rqs_lib::hdl::outbound: Cannot process: consent denied: Reject
                    }
                    State::Cancelled => {
                        progress_bar.set_visible(false);
                        cancel_transfer_button.set_visible(false);
                        eta_label.set_visible(false);
                        result_label.set_visible(false);

                        // Resetting state, permitting removal
                        // Remove immediately here if endpoint info is reset?
                        model_item.set_channel_message(objects::ChannelMessage::default());
                        set_row_activatable(model_item, &clickable_row_to_send, true);
                    }
                    State::Finished => {
                        cancel_transfer_button.set_visible(false);
                        set_row_activatable(model_item, &clickable_row_to_send, false);
                        progress_bar.set_visible(false);
                        eta_label.set_visible(false);

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

                        result_label.set_visible(true);
                        result_label.set_label(&finished_text);
                        result_label.set_css_classes(&["caption", "accent"]);
                    }
                };
            }
        }
    ));

    root_bin
}
