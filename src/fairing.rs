use async_compression::Level;
use lazy_static::lazy_static;
use rocket::{
    fairing::{Fairing, Info, Kind},
    http::{hyper::header::CONTENT_ENCODING, Header, MediaType},
    tokio::{
        io::{AsyncRead, ReadBuf},
        sync::RwLock,
    },
    Request, Response,
};
use std::{collections::HashMap, io::Cursor, task::Poll};

use crate::{CompressionUtils, Encoding};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CachedEncoding {
    Gzip,
    Brotli,
}

lazy_static! {
    static ref EXCLUSIONS: Vec<MediaType> = vec![
        MediaType::parse_flexible("application/gzip").unwrap(),
        MediaType::parse_flexible("application/zip").unwrap(),
        MediaType::parse_flexible("image/*").unwrap(),
        MediaType::parse_flexible("video/*").unwrap(),
        MediaType::parse_flexible("application/wasm").unwrap(),
        MediaType::parse_flexible("application/octet-stream").unwrap(),
    ];
    static ref CACHED_FILES: RwLock<HashMap<(String, CachedEncoding), &'static [u8]>> = {
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
pub struct Compression(pub Level);

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
        Compression(Level::Default)
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
        super::CompressionUtils::compress_response(request, response, &EXCLUSIONS, self.0);
    }
}

/// Compresses all responses with Brotli or Gzip compression. Caches compressed
/// response bodies in memory for selected file types/path suffixes, useful for
/// compressing large compiled JS/CSS files, OTF font packs, etc.  Note that all
/// cached files are held in memory indefinitely.
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
///     .attach(CachedCompression {
///         cached_paths: vec![
///             "".to_owned(),
///             "/".to_owned(),
///             "/about".to_owned(),
///             "/people".to_owned(),
///             "/posts".to_owned(),
///             "/events".to_owned(),
///             "/groups".to_owned(),
///         ],
///         cached_path_prefixes: vec!["/user/".to_owned(), "/g/".to_owned(), "/p/".to_owned()],
///         cached_path_suffixes: vec![".otf".to_owned(), "main.dart.js".to_owned()],
///         ..Default::default()
///     })
///     // ...
///     # ;
/// ```
///
///
#[derive(Default)]
pub struct CachedCompression {
    pub cached_paths: Vec<String>,
    pub cached_path_prefixes: Vec<String>,
    pub cached_path_suffixes: Vec<String>,
    pub excluded_path_prefixes: Vec<String>,
    pub level: Option<Level>,
}

impl CachedCompression {
    /// Caches only the specific paths provided.
    pub fn exact_path_fairing(cached_paths: Vec<String>) -> CachedCompression {
        CachedCompression {
            cached_paths,
            ..Default::default()
        }
    }

    /// Caches all paths with the provided suffixes.
    pub fn path_suffix_fairing(cached_path_suffixes: Vec<String>) -> CachedCompression {
        CachedCompression {
            cached_path_suffixes,
            ..Default::default()
        }
    }

    /// Caches all paths with the provided suffixes.
    pub fn path_prefix_fairing(cached_path_prefixes: Vec<String>) -> CachedCompression {
        CachedCompression {
            cached_path_prefixes,
            ..Default::default()
        }
    }

    /// Caches compressed responses for all paths except those with the excluded prefixes.
    pub fn excluded_path_prefix_fairing(excluded_path_prefixes: Vec<String>) -> CachedCompression {
        CachedCompression {
            cached_path_prefixes: vec!["".to_string()],
            excluded_path_prefixes,
            ..Default::default()
        }
    }

    /// Caches Vec<&str> to Vec<String>.
    pub fn static_paths(paths: Vec<&str>) -> Vec<String> {
        paths.into_iter().map(Into::into).collect()
    }
}

/// When performing cached compression on a body, it is possible that reading the existing body will fail.  We can't return an error directly from a fairing, so we forward the
/// error on to the response by setting in this dummy body which just returns the error.
struct ErrorBody(Option<std::io::Error>);

impl AsyncRead for ErrorBody {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let err = match self.0.take() {
            Some(err) => err,
            None => std::io::Error::new(std::io::ErrorKind::Other, "ErrorBody already read"),
        };
        Poll::Ready(Err(err))
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
        let excluded_from_cache = self
            .excluded_path_prefixes
            .iter()
            .any(|s| path.starts_with(s));
        let cache_compressed_responses = !excluded_from_cache
            && (self.cached_paths.iter().any(|s| path.eq(s))
                || self.cached_path_suffixes.iter().any(|s| path.ends_with(s))
                || self
                    .cached_path_prefixes
                    .iter()
                    .any(|s| path.starts_with(s)));
        if !cache_compressed_responses {
            return;
        }

        let (accepts_gzip, accepts_br) = CompressionUtils::accepted_algorithms(request);
        if !accepts_gzip && !accepts_br {
            return;
        }

        if CompressionUtils::already_encoded(response) {
            return;
        }

        let content_type = response.content_type();
        if CompressionUtils::skip_encoding(&content_type, &EXCLUSIONS) {
            return;
        }

        let desired_encoding = if accepts_br {
            CachedEncoding::Brotli
        } else {
            CachedEncoding::Gzip
        };
        let encoding = match desired_encoding {
            CachedEncoding::Gzip => Encoding::Gzip,
            CachedEncoding::Brotli => Encoding::Brotli,
        };

        if cache_compressed_responses && (accepts_gzip || accepts_br) {
            let cached_body = {
                let guard = CACHED_FILES.read().await;
                let body = guard.get(&(path.clone(), desired_encoding)).copied();
                drop(guard);
                body
            };

            if let Some(cached_body) = cached_body {
                debug!("Found cached response for {}", path);
                response.set_header(Header::new(
                    CONTENT_ENCODING.as_str(),
                    format!("{}", encoding),
                ));
                response.set_sized_body(cached_body.len(), Cursor::new(cached_body));
                return;
            }
        }

        let body = response.body_mut().take();
        let compressed_body: Vec<u8> = match CompressionUtils::compress_body(
            body,
            desired_encoding,
            self.level.unwrap_or(Level::Default),
        )
        .await
        {
            Ok(compressed_body) => compressed_body,
            Err(err) => {
                error!("Failed to compress response body for {}; underlying `AsyncRead` likely failed: {}", path, err);
                response.set_streamed_body(ErrorBody(Some(err)));
                return;
            }
        };
        response.set_header(Header::new(
            CONTENT_ENCODING.as_str(),
            format!("{}", encoding),
        ));
        response.set_sized_body(compressed_body.len(), Cursor::new(compressed_body.clone()));

        debug!("Setting cached response for {}", path);
        CACHED_FILES
            .write()
            .await
            .insert((path, desired_encoding), Vec::leak(compressed_body));
    }
}
