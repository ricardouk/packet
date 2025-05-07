use adw::prelude::*;
use adw::subclass::prelude::*;
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
    #[properties(wrapper_type = super::FileTransferObject)]
    pub struct FileTransferObject {
        #[property(get, set)]
        transfer_kind: RefCell<TransferKind>,
        #[property(get, set)]
        transfer_state: RefCell<State>,
        #[property(get, set)]
        endpoint_info: RefCell<EndpointInfo>,
        #[property(get, set)]
        filenames: RefCell<Vec<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileTransferObject {
        const NAME: &'static str = "FileTransferObject";
        type Type = super::FileTransferObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for FileTransferObject {}
}

glib::wrapper! {
    pub struct FileTransferObject(ObjectSubclass<imp::FileTransferObject>);
}

impl FileTransferObject {
    pub fn new(kind: TransferKind) -> Self {
        let obj: Self = glib::Object::builder().build();
        obj.set_transfer_kind(kind);

        obj
    }
}

impl Default for FileTransferObject {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}
