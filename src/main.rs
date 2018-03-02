extern crate cairo;
extern crate clap;

extern crate gdk;
extern crate glib;
extern crate gstreamer;
extern crate gtk;
extern crate image;

#[macro_use]
extern crate lazy_static;

use clap::{Arg, App};

use gtk::Builder;

mod ui;
use ui::MainController;

mod media;
mod metadata;

fn main() {
    let matches = App::new("media-toc-player")
        .version("0.0.1")
        .author("François Laignel <fengalin@free.fr>")
        .about("A media player with a table of contents")
        .arg(Arg::with_name("INPUT")
            .help("Path to the input media file to play")
            .index(1))
        .get_matches();

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

    if let Some(input_file) = matches.value_of("INPUT") {
        main_ctrl
            .borrow_mut()
            .open_media(input_file.into());
    }

    gtk::main();
}
