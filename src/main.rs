use std::time::Duration;

use crate::built_info::built_time;

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));

    pub fn built_time() -> built::chrono::DateTime<built::chrono::Local> {
        built::util::strptime(BUILT_TIME_UTC).with_timezone(&built::chrono::offset::Local)
    }
}

struct MagickContext;

impl MagickContext {
    pub fn new() -> MagickContext {
        magick_rust::magick_wand_genesis();
        MagickContext
    }
}

impl Drop for MagickContext {
    fn drop(&mut self) {
        magick_rust::magick_wand_terminus();
    }
}

mod data;
pub(crate) mod debug;
mod errors;
mod fields;
mod state;
mod tasks;
mod ui;

fn main() -> Result<(), impl std::error::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("Hello, world!");
    let build_time = built_time();
    println!("This program was built at {build_time}");

    let _magick_context = MagickContext::new();
    let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");

    let _enter = runtime.enter();
    std::thread::spawn(move || {
        runtime.block_on(async {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        })
    });

    ui::App::new().run()
}
