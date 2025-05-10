mod file_card;
mod recipient_card;
mod transfer_card;

use std::{collections::HashMap, sync::Arc};

pub use file_card::*;
use gtk::{
    gio::{self, prelude::ListModelExt},
    glib::object::Cast,
};
pub use recipient_card::*;
use tokio::sync::Mutex;
pub use transfer_card::*;

use crate::objects::DataTransferObject;

// FIXME: Didn't consider the case of an user exiting the Recipient page/dialog
// while a transfer is in motion. Applies to the Receive files dialog/page as well.
// Something needs to be done to allow users to be able to inspect those ongoing
// transfers after they've closed dialog, etc.
pub fn clear_data_transfer_cards(
    model: &gio::ListStore,
    id_cache: &Arc<Mutex<HashMap<String, DataTransferObject>>>,
) {
    use rqs_lib::State;
    let mut pos = 0;
    while let Some(obj) = model.item(pos) {
        let obj = obj.downcast_ref::<DataTransferObject>().unwrap();
        let channel_message = obj.channel_message();
        match channel_message
            .state
            .as_ref()
            .unwrap_or(&rqs_lib::State::Initial)
        {
            State::ReceivingFiles | State::SendingFiles => pos += 1,
            _ => {
                id_cache.blocking_lock().remove(&channel_message.id);
                model.remove(pos);
            }
        }
    }
}
