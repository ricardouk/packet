use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::impl_deref_for_newtype;

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
    // FIXME: Check text type once hdl::TextPayloadType is exported
    // kind: rqs_lib::hdl::TextPayloadType
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
                })
            })
        })
    }
}

#[derive(Debug, Clone, Default, glib::Boxed)]
#[boxed_type(name = "TransferKindBoxed")]
pub enum TransferKind {
    #[default]
    Receive,
    Send,
}

mod imp {
    use std::cell::RefCell;

    use gtk::glib::Properties;

    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::DataTransferObject)]
    pub struct DataTransferObject {
        #[property(get, set)]
        transfer_kind: RefCell<TransferKind>,
        #[property(get, set)]
        transfer_state: RefCell<State>,
        #[property(get, set)]
        endpoint_info: RefCell<EndpointInfo>,
        #[property(get, set)]
        channel_message: RefCell<ChannelMessage>,
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
}

impl Default for DataTransferObject {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}
