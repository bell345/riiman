use crate::built_info::built_time;

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));

    pub fn built_time() -> built::chrono::DateTime<built::chrono::Local> {
        built::util::strptime(BUILT_TIME_UTC)
            .with_timezone(&built::chrono::offset::Local)
    }
}

mod ui;
mod data;
mod state;
mod tasks;

#[tokio::main]
async fn main() -> Result<(), impl std::error::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("Hello, world!");
    let build_time = built_time();
    println!("This program was built at {build_time}");

    ui::App::default().run()
}
