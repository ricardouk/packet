use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use rqs_lib::hdl::TextPayloadType;

use crate::{impl_deref_for_newtype, utils};

#[derive(Debug, Clone, Default, glib::Boxed)]
#[boxed_type(name = "StateBoxed")]
pub struct State(pub rqs_lib::State);
impl_deref_for_newtype!(State, rqs_lib::State);

#[derive(Debug, Clone, Default, glib::Boxed)]
#[boxed_type(name = "EndpointInfoBoxed")]
pub struct EndpointInfo(pub rqs_lib::EndpointInfo);
impl_deref_for_newtype!(EndpointInfo, rqs_lib::EndpointInfo);

#[derive(Debug, Clone, Default, glib::Boxed)]
#[boxed_type(name = "ChannelMessageBoxed")]
pub struct ChannelMessage(pub rqs_lib::channel::ChannelMessage);
impl_deref_for_newtype!(ChannelMessage, rqs_lib::channel::ChannelMessage);

#[derive(Debug, Clone)]
pub struct TextData {
    pub description: String,
    pub text: String,
    pub kind: Option<TextPayloadType>,
}

impl ChannelMessage {
    pub fn get_device_name(channel_message: &rqs_lib::channel::ChannelMessage) -> String {
        channel_message
            .meta
            .as_ref()
            .and_then(|meta| meta.source.as_ref())
            .map(|source| source.name.clone())
            .unwrap_or(gettext("Unknown device"))
    }

    pub fn get_filenames(&self) -> Option<Vec<String>> {
        self.0.meta.as_ref().and_then(|it| it.files.clone())
    }

    pub fn get_text_data(&self) -> Option<TextData> {
        self.0.meta.as_ref().and_then(|meta| {
            meta.text_description.as_ref().and_then(|description| {
                Some(TextData {
                    description: description.clone(),
                    text: meta.text_payload.clone().unwrap_or_default(),
                    kind: meta.text_type.clone(),
                })
            })
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, glib::Boxed)]
#[boxed_type(name = "TransferKindBoxed")]
pub enum TransferKind {
    #[default]
    Receive,
    Send,
}

#[derive(Debug, Clone, Default, PartialEq, glib::Boxed)]
#[boxed_type(name = "TransferStateBoxed")]
pub enum TransferState {
    #[default]
    AwaitingConsentOrIdle,
    RequestedForConsent,
    OngoingTransfer,
    Failed,
    Done,
}

pub mod imp {
    use std::{cell::RefCell, rc::Rc};

    use gtk::glib::Properties;

    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::DataTransferObject)]
    pub struct DataTransferObject {
        pub eta_estimator: Rc<RefCell<utils::DataTransferEta>>,
        pub files_to_send: Rc<RefCell<Vec<String>>>,
        // For modifying widget by listening for events
        #[property(get, set)]
        endpoint_info: RefCell<EndpointInfo>,
        #[property(get, set)]
        channel_message: RefCell<ChannelMessage>,
        // For easier bindings
        #[property(get, set)]
        transfer_kind: RefCell<TransferKind>,
        #[property(get, set)]
        transfer_state: RefCell<TransferState>,
        #[property(get, set)]
        device_name: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DataTransferObject {
        const NAME: &'static str = "DataTransferObject";
        type Type = super::DataTransferObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for DataTransferObject {}
}

glib::wrapper! {
    pub struct DataTransferObject(ObjectSubclass<imp::DataTransferObject>);
}

impl DataTransferObject {
    pub fn new(kind: TransferKind) -> Self {
        let obj: Self = glib::Object::builder().build();
        obj.set_transfer_kind(kind);

        obj
    }
    pub fn copy(value: DataTransferObject) -> Self {
        let obj = Self::new(value.transfer_kind());
        obj.set_endpoint_info(value.endpoint_info());
        obj.set_channel_message(value.channel_message());
        obj.set_device_name(value.device_name());
        *obj.imp().eta_estimator.borrow_mut() = value.imp().eta_estimator.borrow().clone();
        *obj.imp().files_to_send.borrow_mut() = value.imp().files_to_send.borrow().clone();

        obj
    }
}

impl Default for DataTransferObject {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}
