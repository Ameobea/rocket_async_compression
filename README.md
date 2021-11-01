This is a fork of https://github.com/ameobea/rocket_async_compression.git.

Changes made:
 - Uses rocket 0.5.0-rc.1
 - Removes tokio as a dependency and uses the reexported tokio lib from rocket
 - Simplified readme

# Rocket Async Compression

This library provides response compression in both gzip and brotli formats for the [Rocket](https://rocket.rs/) using the [`async-compression`](https://docs.rs/async-compression/0.3.8/async_compression/) library.

It currently only supports usage with Rocket from git with commit hash `693f4f9ee50057fc735e6e7037e6dee5b485ba10`.  If you want to use a different version, you'll have to fork this library and change `Cargo.toml`.

I'd love to get this merged into Rocket itself eventually since I think it would be a very useful addition that I myself can barely live without in a webserver.

## Installation

Add this to `Cargo.toml`:

```toml
[dependencies]
rocket = "0.5.0-rc.1"
rocket_async_compression = { git = "https://github.com/somehowchris/rocket_async_compression.git" }
```

## Usage


The following example will enable compression on releases cause localhost isn't on a remote network (usually)
```rs
use rocket_async_compression::Compression;

#[launch]
async fn main() {
    let server = rocket::build()
        .mount("/", routes![...]);
        
    if cfg!(debug_assertions) {
        server
    } else {
        server.attach(Compression::fairing())
    }
}
```
