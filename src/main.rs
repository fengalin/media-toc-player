extern crate cairo;
extern crate clap;
extern crate env_logger;
extern crate gdk;
extern crate gettextrs;
extern crate gio;
extern crate glib;
extern crate gstreamer;
extern crate gtk;
extern crate image;

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate nom;

use clap::{App, Arg};

use gettextrs::{gettext, TextDomain, TextDomainError};

use gio::prelude::*;

use std::path::PathBuf;

mod ui;
use ui::{APP_ID, MainController};

mod media;
mod metadata;

fn init_locale() {
    match TextDomain::new("media-toc-player").prepend("target").init() {
        Ok(locale) => info!("Translation found, `setlocale` returned {:?}", locale),
        Err(TextDomainError::TranslationNotFound(lang)) => {
            warn!("Translation not found for language {}", lang)
        }
        Err(TextDomainError::InvalidLocale(locale)) => error!("Invalid locale {}", locale),
    }
}

fn handle_command_line() -> Option<PathBuf> {
    let about_msg = gettext("A media player with a table of contents");
    let help_msg = gettext("Display this message");
    let version_msg = gettext("Print version information");

    let input_arg = gettext("MEDIA");

    App::new("media-toc-player")
        .version(env!("CARGO_PKG_VERSION"))
        .author("François Laignel <fengalin@free.fr>")
        .about(about_msg.as_str())
        .help_message(help_msg.as_str())
        .version_message(version_msg.as_str())
        .arg(
            Arg::with_name(input_arg.as_str())
                .help(&gettext("Path to the input media file"))
                .last(false),
        )
        .get_matches()
        .value_of(input_arg.as_str())
        .map(|input_file| input_file.into())
}

fn run(is_gst_ok: bool, input_file: Option<PathBuf>) {
    // Init App
    let gtk_app = gtk::Application::new(APP_ID, gio::ApplicationFlags::empty())
        .expect("Failed to initialize GtkApplication");

    gtk_app.connect_startup(move |gtk_app| {
        let main_ctrl = MainController::new(gtk_app, is_gst_ok);
        let input_file = input_file.clone();
        gtk_app.connect_activate(move |_| {
            let mut main_ctrl = main_ctrl.borrow_mut();
            main_ctrl.show_all();

            if is_gst_ok {
                if let Some(ref input_file) = input_file {
                    // FIXME: there must be a lifetime way to avoid
                    // all these duplications
                    main_ctrl.open_media(input_file.clone());
                }
            }
        });
    });

    gtk_app.run(&[]);
}

fn main() {
    env_logger::init();

    init_locale();

    // Messages are not translated unless gtk (glib) is initialized
    let is_gtk_ok = gtk::init().is_ok();

    let input_file = handle_command_line();

    if is_gtk_ok {
        run(gstreamer::init().is_ok(), input_file);
    } else {
        error!("{}", gettext("Failed to initialize GTK"));
    }
}
