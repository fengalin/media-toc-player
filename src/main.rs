extern crate gdk;
extern crate glib;
extern crate gstreamer;
extern crate gtk;

#[macro_use]
extern crate lazy_static;

use gtk::Builder;

mod ui;
use ui::MainController;

mod media;
mod metadata;

fn main() {
    if gtk::init().is_err() {
        panic!("Failed to initialize GTK.");
    }

    gstreamer::init().unwrap();

    // TODO: there's a `Settings` struct in GTK:
    // https://github.com/gtk-rs/gtk/blob/master/src/auto/settings.rs

    let main_ctrl = {
        let builder = Builder::new_from_string(include_str!("ui/media-toc-player.ui"));
        MainController::new(&builder)
    };
    main_ctrl.borrow().show_all();

    gtk::main();
}
