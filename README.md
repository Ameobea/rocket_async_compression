# Rocket Async Compression

This library provides response compression in both gzip and brotli formats for the [Rocket](https://rocket.rs/) using the [`async-compression`](https://docs.rs/async-compression/0.3.8/async_compression/) library.

It currently only supports usage with Rocket from git with commit hash `693f4f9ee50057fc735e6e7037e6dee5b485ba10`.  If you want to use a different version, you'll have to fork this library and change `Cargo.toml`.

## Installation

Add this to `Cargo.toml`:

```toml
[dependencies]
rocket = { git = "https://github.com/SergioBenitez/Rocket.git", rev = "693f4f9ee50057fc735e6e7037e6dee5b485ba10" }
rocket_async_compression = "0.1"
```

## Usage

```rs
#[rocket::main]
async fn main() {
    rocket::build()
        .mount("/", routes![...])
        // Attach compression fairing here
        .attach(rocket_async_compression::Compression::fairing())
        .ignite()
        .await
        .expect("Error starting Rocket")
        .launch()
        .await
        .expect("Error running Rocket");

    println!("Exited cleanly");
}
```
