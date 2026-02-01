#[macro_use]
extern crate rocket;

use rocket::fs::{FileServer, relative};
use rocket_async_compression::CachedCompression;

#[launch]
async fn rocket() -> _ {
    rocket::build()
        .mount(
            "/",
            FileServer::new(relative!("examples/cached-compression/static")),
        )
        .attach(
            CachedCompression::builder()
                .cached_path_suffixes(CachedCompression::static_paths(vec![".txt"]))
                .build(),
        )
}
