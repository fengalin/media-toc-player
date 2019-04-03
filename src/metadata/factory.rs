use std::boxed::Box;

use super::{Format, MKVMergeTextFormat, Reader};

pub struct Factory {}

impl Factory {
    pub fn get_extensions() -> Vec<(&'static str, Format)> {
        let mut result = Vec::<(&'static str, Format)>::new();

        // Only MKVMergeTextFormat implemented for Read ATM
        result.push((MKVMergeTextFormat::get_extension(), Format::MKVMergeText));

        result
    }

    pub fn get_reader(format: Format) -> Box<dyn Reader> {
        match format {
            Format::MKVMergeText => MKVMergeTextFormat::new_as_boxed(),
        }
    }
}
