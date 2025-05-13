use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::{self};

use crate::{objects, utils};

pub mod imp {
    use std::{cell::RefCell, rc::Rc};

    use gtk::glib::Properties;

    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::ShareRequestState)]
    pub struct ShareRequestState {
        pub eta: Rc<RefCell<utils::DataTransferEta>>,
        #[property(get, set)]
        event: RefCell<objects::ChannelMessage>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ShareRequestState {
        const NAME: &'static str = "ShareRequestState";
        type Type = super::ShareRequestState;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ShareRequestState {}
}

glib::wrapper! {
    pub struct ShareRequestState(ObjectSubclass<imp::ShareRequestState>);
}

impl Default for ShareRequestState {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}

impl ShareRequestState {
    pub fn new(msg: &objects::ChannelMessage) -> Self {
        let obj: Self = Default::default();
        obj.set_event(msg);

        obj
    }
}
