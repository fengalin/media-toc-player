mod chapter_tree_manager;
use self::chapter_tree_manager::{ChapterTreeManager, PositionStatus};

mod image;
use self::image::Image;

mod info_bar_controller;
use self::info_bar_controller::InfoBarController;

mod info_controller;
use self::info_controller::InfoController;
mod info_dispatcher;
use self::info_dispatcher::InfoDispatcher;

pub mod main_controller;
pub use self::main_controller::{ControllerState, MainController};
mod main_dispatcher;
pub use self::main_dispatcher::MainDispatcher;

mod perspective_controller;
use self::perspective_controller::PerspectiveController;
mod perspective_dispatcher;
use self::perspective_dispatcher::PerspectiveDispatcher;

mod streams_controller;
use self::streams_controller::StreamsController;
mod streams_dispatcher;
use self::streams_dispatcher::StreamsDispatcher;

mod ui_event;
use self::ui_event::{UIEventHandler, UIEventSender, UIFocusContext};

mod video_controller;
use self::video_controller::VideoController;
mod video_dispatcher;
use self::video_dispatcher::VideoDispatcher;

use futures::channel::mpsc as async_mpsc;
use gio;
use gio::prelude::*;
use gtk;
use log::warn;

use std::{cell::RefCell, rc::Rc};

use crate::{
    application::{CommandLineArguments, APP_ID},
    media::{self, PlaybackPipeline},
    metadata,
};

#[macro_export]
macro_rules! spawn {
    ($future:expr) => {
        glib::MainContext::ref_thread_default().spawn_local($future);
    };
}

fn register_resource(resource: &[u8]) {
    let gbytes = glib::Bytes::from(resource);
    gio::Resource::new_from_data(&gbytes)
        .and_then(|resource| {
            gio::resources_register(&resource);
            Ok(())
        })
        .unwrap_or_else(|err| {
            warn!("unable to load resources: {:?}", err);
        });
}

pub fn run(args: CommandLineArguments) {
    register_resource(include_bytes!("../../target/resources/ui.gresource"));

    let gtk_app = gtk::Application::new(Some(&APP_ID), gio::ApplicationFlags::empty())
        .expect("Failed to initialize GtkApplication");

    gtk_app.connect_activate(move |gtk_app| MainController::setup(gtk_app, &args));
    gtk_app.run(&[]);
}

type MediaEventReceiver = async_mpsc::Receiver<media::MediaEvent>;

pub trait UIController {
    fn new_media(&mut self, _pipeline: &PlaybackPipeline) {}
    fn cleanup(&mut self);
    fn streams_changed(&mut self, _info: &metadata::MediaInfo) {}
    fn grab_focus(&self) {}
}

pub trait UIDispatcher {
    type Controller: UIController;

    fn setup(
        ctrl: &mut Self::Controller,
        main_ctrl_rc: &Rc<RefCell<MainController>>,
        app: &gtk::Application,
        ui_event: &UIEventSender,
    );

    // bind context specific accels
    fn bind_accels_for(_ctx: UIFocusContext, _app: &gtk::Application) {}
}
