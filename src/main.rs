extern crate cairo;
extern crate clap;

extern crate gdk;
extern crate gettextrs;
extern crate glib;
extern crate gstreamer;
extern crate gtk;
extern crate image;

#[macro_use]
extern crate lazy_static;

use clap::{Arg, App};

use gettextrs::{TextDomain, TextDomainError, gettext};

use gtk::Builder;

mod ui;
use ui::MainController;

mod media;
mod metadata;

fn main() {
    let locale = {
        match TextDomain::new("media-toc-player").prepend("target").init() {
            Ok(locale) => {
                format!("translation found, `setlocale` returned {:?}", locale)
            }
            Err(TextDomainError::TranslationNotFound(lang)) => {
                format!("translation not found for language {}", lang)
            }
            Err(TextDomainError::InvalidLocale(locale)) => {
                format!("Invalid locale {}", locale)
            }
        }
    };

    // Messages are not translated unless gtk (glib) is initialized
    let is_gtk_ok = gtk::init().is_ok();

    let about_msg = gettext("A media player with a table of contents");
    let help_msg = gettext("Display this message");
    let version_msg = gettext("Print version information");

    let input_arg = gettext("MEDIA");

    let matches = App::new("media-toc-player")
        .version("0.0.1")
        .author("François Laignel <fengalin@free.fr>")
        .about(about_msg.as_str())
        .help_message(help_msg.as_str())
        .version_message(version_msg.as_str())
        .arg(Arg::with_name(input_arg.as_str())
            .help(&gettext("Path to the input media file"))
            .last(false))
        .get_matches();

    if gtk::init().is_err() {
        panic!("Failed to initialize GTK.");
    }

    if !is_gtk_ok {
        panic!(gettext("Failed to initialize GTK"));
    }
    gstreamer::init().unwrap();

    println!("Locale: {}", locale);

    // TODO: there's a `Settings` struct in GTK:
    // https://github.com/gtk-rs/gtk/blob/master/src/auto/settings.rs

    let main_ctrl = {
        let builder = Builder::new_from_string(include_str!("ui/media-toc-player.ui"));
        MainController::new(&builder)
    };
    main_ctrl.borrow().show_all();

    if let Some(input_file) = matches.value_of(input_arg.as_str()) {
        main_ctrl
            .borrow_mut()
            .open_media(input_file.into());
    }

    gtk::main();
}
