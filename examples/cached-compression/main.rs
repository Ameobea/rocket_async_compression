#[macro_use]
extern crate rocket;

use rocket::fs::{relative, FileServer};
use rocket_async_compression::CachedCompression;

#[launch]
async fn rocket() -> _ {
    rocket::build()
        .mount(
            "/",
            FileServer::from(relative!("examples/cached-compression/static")),
        )
        .attach(CachedCompression {
            cached_paths: vec![],
            cached_path_endings: vec![".txt"],
        })
}
