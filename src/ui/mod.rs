mod info_controller;
use self::info_controller::InfoController;

mod image_surface;
use self::image_surface::ImageSurface;

pub mod main_controller;
pub use self::main_controller::{ControllerState, MainController};

mod chapter_tree_manager;
use self::chapter_tree_manager::ChapterTreeManager;

mod streams_controller;
use self::streams_controller::StreamsController;

mod video_controller;
use self::video_controller::VideoController;
