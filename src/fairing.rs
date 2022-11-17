use lazy_static::lazy_static;
use rocket::tokio::{io::AsyncReadExt, sync::RwLock};
use rocket::{
    fairing::{Fairing, Info, Kind},
    http::{hyper::header::CONTENT_ENCODING, MediaType},
    Request, Response,
};
use std::{collections::HashMap, io::Cursor};

use crate::CompressionUtils;

lazy_static! {
    static ref EXCLUSIONS: Vec<MediaType> = vec![
        MediaType::parse_flexible("application/gzip").unwrap(),
        MediaType::parse_flexible("application/zip").unwrap(),
        MediaType::parse_flexible("image/*").unwrap(),
        MediaType::parse_flexible("video/*").unwrap(),
        MediaType::parse_flexible("application/wasm").unwrap(),
        MediaType::parse_flexible("application/octet-stream").unwrap(),
    ];
    static ref CACHED_FILES: RwLock<HashMap<(String, bool, bool), (Vec<u8>, String)>> = {
        let m = HashMap::new();
        RwLock::new(m)
    };
}

/// Compresses all responses with Brotli or Gzip compression.
///
/// Compression is done in the same manner as the [`Compress`](super::Compress)
/// responder.
///
/// By default, the fairing does not compress responses with a `Content-Type`
/// matching any of the following:
///
/// - `application/gzip`
/// - `application/zip`
/// - `image/*`
/// - `video/*`
/// - `application/wasm`
/// - `application/octet-stream`
///
/// The excluded types can be changed changing the `compress.exclude` Rocket
/// configuration property in Rocket.toml. The default `Content-Type` exclusions
/// will be ignored if this is set, and must be added back in one by one if
/// desired.
///
/// ```toml
/// [global.compress]
/// exclude = ["video/*", "application/x-xz"]
/// ```
///
/// # Usage
///
/// Attach the compression [fairing](/rocket/fairing/) to your Rocket
/// application:
///
/// ```rust
///
/// use rocket_async_compression::Compression;
///
///
/// rocket::build()
///     // ...
///     .attach(Compression::fairing())
///     // ...
///     # ;
///
/// ```
pub struct Compression(());

impl Compression {
    /// Returns a fairing that compresses outgoing requests.
    ///
    /// ## Example
    /// To attach this fairing, simply call `attach` on the application's
    /// `Rocket` instance with `Compression::fairing()`:
    ///
    /// ```rust
    ///
    /// use rocket_async_compression::Compression;
    ///
    /// rocket::build()
    ///     // ...
    ///     .attach(Compression::fairing())
    ///     // ...
    ///     # ;
    /// ```
    pub fn fairing() -> Compression {
        Compression(())
    }
}

#[rocket::async_trait]
impl Fairing for Compression {
    fn info(&self) -> Info {
        Info {
            name: "Response compression",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        super::CompressionUtils::compress_response(request, response, &EXCLUSIONS);
    }
}

/// Compresses all responses with Brotli or Gzip compression. Caches compressed
/// response bodies in memory for selected file types/path suffixes, useful for
/// compressing large compiled JS/CSS files, OTF font packs, etc.
///
/// Compression is done in the same manner as the [`Compression`](Compression)
/// fairing.
///
/// # Usage
///
/// Attach the compression [fairing](/rocket/fairing/) to your Rocket
/// application:
///
/// ```rust
///
/// use rocket_async_compression::CachedCompression;
///
/// rocket::build()
///     // ...
///     .attach(CachedCompression::fairing(vec![".otf", "main.dart.js"]))
///     // ...
///     # ;
///
/// ```
pub struct CachedCompression {
    pub cached_path_endings: Vec<&'static str>,
}

impl CachedCompression {
    pub fn fairing(cached_path_endings: Vec<&'static str>) -> CachedCompression {
        CachedCompression {
            cached_path_endings,
        }
    }
}

#[rocket::async_trait]
impl Fairing for CachedCompression {
    fn info(&self) -> Info {
        Info {
            name: "Cached response compression",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        let path = request.uri().path().to_string();
        let cache_compressed_responses = self.cached_path_endings.iter().any(|s| path.ends_with(s));
        let (accepts_gzip, accepts_br) = CompressionUtils::accepted_algorithms(request);

        if cache_compressed_responses {
            let guard = CACHED_FILES.read().await;
            if let Some((cached_body, header)) =
                guard.get(&(path.clone(), accepts_gzip, accepts_br))
            {
                response.set_header(rocket::http::Header::new(
                    CONTENT_ENCODING.as_str(),
                    header.clone(),
                ));
                let body = cached_body.clone();
                response.set_sized_body(body.len(), Cursor::new(body));
                return;
            } else {
                drop(guard);
            }
        }

        super::CompressionUtils::compress_response(request, response, &EXCLUSIONS);

        if !cache_compressed_responses {
            return;
        }

        let mut compressed_body: Vec<u8> = vec![];
        match response.body_mut().read_to_end(&mut compressed_body).await {
            Err(_) => return,
            _ => (),
        }
        response.set_sized_body(compressed_body.len(), Cursor::new(compressed_body.clone()));
        let header = response
            .headers()
            .get_one(CONTENT_ENCODING.as_str())
            .unwrap()
            .to_string();
        CACHED_FILES
            .write()
            .await
            .insert((path, accepts_gzip, accepts_br), (compressed_body, header));
    }
}
