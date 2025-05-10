use adw::prelude::*;
use adw::subclass::prelude::*;
use formatx::formatx;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib, glib::clone};

use crate::window::QuickShareApplicationWindow;

pub fn create_file_card(
    win: &QuickShareApplicationWindow,
    model: &gio::ListStore,
    model_item: &gio::File,
) -> adw::Bin {
    let imp = win.imp();

    let root_bin = adw::Bin::new();
    let _box = gtk::Box::builder().build();
    let root_box = gtk::Box::builder()
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .spacing(12)
        .build();
    root_bin.set_child(Some(&_box));
    _box.append(&root_box);

    let file_avatar = adw::Avatar::builder()
        .icon_name("folder-templates-symbolic")
        .size(48)
        .build();
    root_box.append(&file_avatar);

    let filename_label = gtk::Label::builder()
        .label(model_item.basename().unwrap().to_string_lossy())
        .xalign(0.)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .build();
    root_box.append(&filename_label);

    let remove_file_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .icon_name("cross-large-symbolic")
        .css_classes(["flat", "circular"])
        .tooltip_text(&gettext("Remove"))
        .build();
    root_box.append(&remove_file_button);

    remove_file_button.connect_clicked(clone!(
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

            imp.manage_files_count_label
                .set_label(&formatx!(gettext("{} Files"), model.n_items() as usize).unwrap());

            if model.n_items() == 0 {
                imp.main_nav_view.pop();
            }
        }
    ));

    root_bin
}
