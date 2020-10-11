use gettextrs::gettext;

use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;

use log::{error, info};

use super::{UIEventSender, UIFocusContext};

pub struct InfoBarController {
    info_bar: gtk::InfoBar,
    revealer: gtk::Revealer,
    label: gtk::Label,
    ui_event: UIEventSender,
}

impl InfoBarController {
    pub fn new(app: &gtk::Application, builder: &gtk::Builder, ui_event: &UIEventSender) -> Self {
        let info_bar: gtk::InfoBar = builder.get_object("info_bar").unwrap();
        info_bar.add_button(&gettext("No"), gtk::ResponseType::No);
        info_bar.add_button(&gettext("Yes to all"), gtk::ResponseType::Apply);
        info_bar.add_button(&gettext("Cancel"), gtk::ResponseType::Cancel);
        info_bar.set_default_response(gtk::ResponseType::Yes);

        let revealer: gtk::Revealer = builder.get_object("info_bar-revealer").unwrap();

        let close_info_bar_action = gio::SimpleAction::new("close_info_bar", None);
        app.add_action(&close_info_bar_action);
        app.set_accels_for_action("app.close_info_bar", &["Escape"]);

        info_bar.connect_response(clone!(@strong ui_event => move |_, _| {
            ui_event.hide_info_bar();
            ui_event.restore_context();
        }));

        if gstreamer::init().is_ok() {
            close_info_bar_action
                .connect_activate(clone!(@strong info_bar => move |_, _| info_bar.emit_close()));
        } else {
            close_info_bar_action.connect_activate(clone!(@strong ui_event => move |_, _| {
                ui_event.quit();
            }));

            info_bar.connect_response(clone!(@strong ui_event => move |_, _| {
                ui_event.quit();
            }));
        }

        let ui_event = ui_event.clone();
        InfoBarController {
            info_bar,
            revealer,
            label: builder.get_object("info_bar-lbl").unwrap(),
            ui_event,
        }
    }

    pub fn hide(&self) {
        self.revealer.set_reveal_child(false);
    }

    pub fn show_message<Msg: AsRef<str>>(&mut self, type_: gtk::MessageType, message: Msg) {
        self.info_bar.set_show_close_button(true);
        self.info_bar.set_message_type(type_);
        self.label.set_label(message.as_ref());
        self.revealer.set_reveal_child(true);

        self.ui_event.temporarily_switch_to(UIFocusContext::InfoBar);
    }

    pub fn show_error<Msg: AsRef<str>>(&mut self, message: Msg) {
        error!("{}", message.as_ref());
        self.show_message(gtk::MessageType::Error, message);
    }

    pub fn show_info<Msg: AsRef<str>>(&mut self, message: Msg) {
        info!("{}", message.as_ref());
        self.show_message(gtk::MessageType::Info, message);
    }
}
