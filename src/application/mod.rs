use gio;
use gio::prelude::*;
use gtk;

use ui::MainController;

pub const TLD: &str = "org";
pub const SLD: &str = "fengalin";
lazy_static! {
    pub static ref APP_ID: String = format!("{}.{}.{}", TLD, SLD, env!("CARGO_PKG_NAME"));
}

mod command_line;
pub use self::command_line::{handle_command_line, CommandLineArguments};

mod configuration;
pub use self::configuration::CONFIG;

mod locale;
pub use self::locale::init_locale;

pub fn run(is_gst_ok: bool, args: CommandLineArguments) {
    let gtk_app = gtk::Application::new(&APP_ID[..], gio::ApplicationFlags::empty())
        .expect("Failed to initialize GtkApplication");

    gtk_app.connect_activate(move |gtk_app| {
        let main_ctrl = MainController::new(gtk_app, is_gst_ok);
        main_ctrl.borrow().show_all();

        if is_gst_ok {
            if let Some(ref input_file) = args.input_file {
                // FIXME: move `open_media` arg as &Path
                main_ctrl.borrow_mut().open_media(input_file.to_owned());
            }
        }
    });

    gtk_app.run(&[]);
}
