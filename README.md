# Rocket Async Compression
[![Open in Gitpod](https://gitpod.io/button/open-in-gitpod.svg)](https://gitpod.io/#https://github.com/ameobea/rocket_async_compression)

This library provides response compression in both gzip and brotli formats for the [Rocket](https://rocket.rs/) using the [`async-compression`](https://docs.rs/async-compression/0.3.8/async_compression/) library.

It currently supports usage with Rocket `0.5.0-rc.1`.  If you want to use a different version, you'll have to fork this library and change `Cargo.toml`.

> I'd love to get this merged into Rocket itself eventually since I think it would be a very useful addition that I myself can barely live without in a webserver.

## Installation

Add this to `Cargo.toml`:

```toml
[dependencies]
rocket = "0.5.0-rc.1"
rocket_async_compression = "0.1.0"
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

## Contributors
- Casey Primozic ([Ameobea](https://github.com/Ameobea))
- Christof Weickhardt ([somehowchris](https://github.com/somehowchris))
