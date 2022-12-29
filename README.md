# Rocket Async Compression

This library provides response compression in both gzip and brotli formats for the [Rocket](https://rocket.rs/) using the [`async-compression`](https://docs.rs/async-compression/0.3.8/async_compression/) library.

It currently supports usage with Rocket `0.5.0-rc.2`.  If you want to use a different version, you'll have to fork this library and change `Cargo.toml`.

> I'd love to get this merged into Rocket itself eventually since I think it would be a very useful addition that I myself can barely live without in a webserver.

## Installation

Add this to `Cargo.toml`:

```toml
[dependencies]
rocket = "0.5.0-rc.2"
rocket_async_compression = "0.2.0"
```

## Usage

The following example will enable compression only when the crate is built in release mode.  Compression can be very slow when using unoptimized debug builds while developing locally.

```rs
#[macro_use]
extern crate rocket;

use rocket_async_compression::Compression;

#[launch]
async fn rocket() -> _ {
    let server = rocket::build()
        .mount("/", routes![...]);

    if cfg!(debug_assertions) {
        server
    } else {
        server.attach(Compression::fairing())
    }
}
```

### Cached Compression

When serving static files, it can be useful to avoid the work of compressing the same files repeatedly for each request.  This crate provides an alternative `CachedCompression` fairing which stores cached responses in memory and uses those when available.

Note that cached responses do not expire and will be held in memory for the life of the program.  You should only use this fairing for compressing static files that will not change while the server is running.

```rs
#[macro_use]
extern crate rocket;

use rocket::fs::{relative, FileServer};
use rocket_async_compression::CachedCompression;

#[launch]
async fn rocket() -> _ {
    rocket::build()
        .mount(
            "/",
            FileServer::from(relative!("static")),
        )
        .attach(CachedCompression::fairing(vec![".js", ".css", ".html", ".wasm"]))
}
```
