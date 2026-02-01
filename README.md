# Rocket Async Compression

This library provides response compression in both gzip and brotli formats for [Rocket](https://rocket.rs/) using the [`async-compression`](https://docs.rs/async-compression) library.

> I'd love to get this merged into Rocket itself eventually since I think it would be a very useful addition that I myself can barely live without in a webserver.

## Installation

Add this to `Cargo.toml`:

```toml
[dependencies]
rocket = "0.5"
rocket_async_compression = "0.6"
```

## Features

- **Multiple compression algorithms**: Gzip, Brotli, Deflate, and Zstd with configurable compression levels
- **Accept-Encoding q-value support**: Respects client preferences including quality values (e.g., `gzip;q=0.8, br;q=1.0`) and `identity` encoding
- **Cached compression**: Optional in-memory caching for static files with LRU eviction
- **Configurable limits**: Maximum body size, compression timeout, cache capacity, and TTL

## Usage

The following example will enable compression only when the crate is built in release mode. Compression can be very slow when using unoptimized debug builds while developing locally.

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

When serving static files, it can be useful to avoid the work of compressing the same files repeatedly for each request. This crate provides an alternative `CachedCompression` fairing which stores cached responses in memory and uses those when available.

The cache has configurable limits to prevent unbounded memory growth:
- **Maximum capacity**: Default 1000 entries, with LRU eviction when exceeded
- **Time-to-live**: Default 1 hour, after which entries are automatically removed

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
        .attach(
            CachedCompression::builder()
                .cached_path_suffixes(vec![".js".into(), ".css".into(), ".html".into(), ".wasm".into()])
                .build()
        )
}
```

With custom cache settings:

```rs
use std::time::Duration;
use rocket_async_compression::CachedCompression;

CachedCompression::builder()
    .cache_max_capacity(500)           // Maximum 500 cached entries
    .cache_ttl(Duration::from_secs(1800))  // 30 minute TTL
    .cached_path_suffixes(vec![".js".into()])
    .build()
```

### Accept-Encoding Support

The library fully supports the HTTP `Accept-Encoding` header with quality values:

- Selects the encoding with the highest q-value (e.g., `gzip;q=0.5, br;q=1.0` uses Brotli)
- Priority when q-values are equal: zstd > brotli > gzip > deflate (by compression efficiency)
- Respects `identity` encoding - if `identity` has a higher q-value than compression algorithms, no compression is applied
- Treats `q=0` as "not acceptable" for that encoding

### Supported Algorithms

| Algorithm | Content-Encoding | Notes |
|-----------|------------------|-------|
| Zstd | `zstd` | Best compression ratio and speed |
| Brotli | `br` | Excellent compression, widely supported |
| Gzip | `gzip` | Universal browser support |
| Deflate | `deflate` | Legacy support |
