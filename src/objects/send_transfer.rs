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

impl std::fmt::Display for EndpointInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ id={:?} present={:?} name={:?} }}",
            self.id,
            self.present.unwrap_or_default(),
            self.name.as_ref().map(|it| it.as_str()).unwrap_or_default(),
        )
    }
}

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
    pub fn _get_device_name(channel_message: &rqs_lib::channel::ChannelMessage) -> String {
        channel_message
            .meta
            .as_ref()
            .and_then(|meta| meta.source.as_ref())
            .map(|source| source.name.clone())
            .unwrap_or(gettext("Unknown device"))
    }

    pub fn get_device_name(&self) -> String {
        self.meta
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
    Queued,
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
    #[properties(wrapper_type = super::SendRequestState)]
    pub struct SendTransferState {
        pub eta: Rc<RefCell<utils::DataTransferEta>>,
        pub files: Rc<RefCell<Vec<String>>>,

        #[property(get, set)]
        transfer_state: RefCell<TransferState>,
        #[property(get, set)]
        device_name: RefCell<String>,

        // For modifying widget by listening for events
        #[property(get, set)]
        endpoint_info: RefCell<EndpointInfo>,
        #[property(get, set)]
        event: RefCell<ChannelMessage>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SendTransferState {
        const NAME: &'static str = "PacketSendTransferState";
        type Type = super::SendRequestState;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SendTransferState {}
}

glib::wrapper! {
    pub struct SendRequestState(ObjectSubclass<imp::SendTransferState>);
}

impl SendRequestState {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn copy(&self) -> Self {
        let obj = Self::new();
        obj.set_endpoint_info(self.endpoint_info());
        obj.set_event(self.event());
        obj.set_device_name(self.device_name());
        *obj.imp().eta.borrow_mut() = self.imp().eta.borrow().clone();
        *obj.imp().files.borrow_mut() = self.imp().files.borrow().clone();

        obj
    }
}

impl Default for SendRequestState {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}
