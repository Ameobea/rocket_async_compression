use async_compression::Level;
use bytes::Bytes;
use moka::future::Cache;
use rocket::{
    Request, Response,
    fairing::{Fairing, Info, Kind},
    http::{Header, MediaType},
    tokio::io::{AsyncRead, ReadBuf},
};
use std::{io::Cursor, sync::LazyLock, task::Poll, time::Duration};
use tracing::{debug, error, warn};

use crate::{CONTENT_ENCODING, CompressionUtils, Encoding};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CachedEncoding {
    Gzip,
    Brotli,
    Deflate,
    Zstd,
}

/// Default maximum number of cached compressed responses (1000 entries).
pub const DEFAULT_CACHE_MAX_CAPACITY: u64 = 1000;

/// Default time-to-live for cached compressed responses (1 hour).
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(3600);

/// Default maximum body size for compression (50 MiB).
///
/// Bodies larger than this will not be compressed to prevent memory exhaustion.
pub const DEFAULT_MAX_BODY_SIZE: u64 = 50 * 1024 * 1024;

/// Default compression timeout (30 seconds).
///
/// Compression operations taking longer than this will be aborted.
pub const DEFAULT_COMPRESSION_TIMEOUT: Duration = Duration::from_secs(30);

type CacheKey = (String, CachedEncoding);
type CacheValue = Bytes;
type CompressionCache = Cache<CacheKey, CacheValue>;

static EXCLUSIONS: LazyLock<Vec<MediaType>> = LazyLock::new(|| {
    vec![
        MediaType::parse_flexible("application/gzip").unwrap(),
        MediaType::parse_flexible("application/zip").unwrap(),
        MediaType::parse_flexible("image/*").unwrap(),
        MediaType::parse_flexible("video/*").unwrap(),
        MediaType::parse_flexible("application/octet-stream").unwrap(),
        MediaType::parse_flexible("text/event-stream").unwrap(),
    ]
});

fn build_cache(max_capacity: u64, ttl: Duration) -> CompressionCache {
    Cache::builder()
        .max_capacity(max_capacity)
        .time_to_live(ttl)
        .build()
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
/// - `application/octet-stream`
/// - `text/event-stream`
///
/// # Usage
///
/// Attach the compression [fairing](/rocket/fairing/) to your Rocket
/// application:
///
/// ```rust
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
pub struct Compression {
    pub level: Level,
    pub excluded_content_types: Vec<MediaType>,
}

impl Compression {
    /// Returns a fairing that compresses outgoing requests.  Uses default compression level and excluded content types.
    ///
    /// ## Example
    /// To attach this fairing, simply call `attach` on the application's
    /// `Rocket` instance with `Compression::fairing()`:
    ///
    /// ```rust
    /// use rocket_async_compression::Compression;
    ///
    /// rocket::build()
    ///     // ...
    ///     .attach(Compression::fairing());
    ///     // ...
    /// ```
    pub fn fairing() -> Compression {
        Compression::with_level(Level::Default)
    }

    /// Returns a fairing that compresses outgoing requests with the specified
    /// compression level.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use rocket_async_compression::{Compression, Level};
    ///
    /// rocket::build()
    ///    // ...
    ///    .attach(Compression::with_level(Level::Fastest));
    ///    // ...
    /// ```
    pub fn with_level(level: Level) -> Compression {
        Compression {
            level,
            excluded_content_types: EXCLUSIONS.clone(),
        }
    }

    /// Replaces the default list of excluded content types with the provided list.
    pub fn exlude_content_types(self, excluded_content_types: Vec<MediaType>) -> Self {
        Compression {
            excluded_content_types,
            ..self
        }
    }

    /// Returns a mutable reference to the list of excluded content types.
    pub fn excluded_content_types(&mut self) -> &mut Vec<MediaType> {
        &mut self.excluded_content_types
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
        super::CompressionUtils::compress_response(
            request,
            response,
            &self.excluded_content_types,
            self.level,
        );
    }
}

/// Compresses all responses with Brotli or Gzip compression. Caches compressed
/// response bodies in memory for selected file types/path suffixes, useful for
/// compressing large compiled JS/CSS files, OTF font packs, etc.
///
/// Compression is done in the same manner as the [`Compression`](Compression)
/// fairing.
///
/// # Cache Configuration
///
/// The cache has configurable limits to prevent unbounded memory growth:
///
/// - **`cache_max_capacity`**: Maximum number of cached entries. When exceeded,
///   least-recently-used entries are evicted. Default: 1000 entries
///   (see [`DEFAULT_CACHE_MAX_CAPACITY`]).
///
/// - **`cache_ttl`**: Time-to-live for cached entries. Entries are automatically
///   removed after this duration. Default: 1 hour (see [`DEFAULT_CACHE_TTL`]).
///
/// # Usage
///
/// Attach the compression [fairing](/rocket/fairing/) to your Rocket
/// application:
///
/// ```rust
/// use rocket_async_compression::CachedCompression;
///
/// rocket::build()
///     // ...
///     .attach(CachedCompression::builder()
///         .cached_paths(vec!["/".to_owned(), "/about".to_owned()])
///         .cached_path_suffixes(vec![".js".to_owned(), ".css".to_owned()])
///         .build());
///     // ...
/// ```
///
/// With custom cache settings:
///
/// ```rust
/// use std::time::Duration;
/// use rocket_async_compression::CachedCompression;
///
/// rocket::build()
///     // ...
///     .attach(CachedCompression::builder()
///         .cache_max_capacity(500)
///         .cache_ttl(Duration::from_secs(1800)) // 30 minutes
///         .cached_path_suffixes(vec![".js".to_owned()])
///         .build());
///     // ...
/// ```
pub struct CachedCompression {
    cached_paths: Vec<String>,
    cached_path_prefixes: Vec<String>,
    cached_path_suffixes: Vec<String>,
    excluded_path_prefixes: Vec<String>,
    level: Option<Level>,
    max_body_size: u64,
    compression_timeout: Duration,
    cache: CompressionCache,
}

/// Builder for [`CachedCompression`].
///
/// Use [`CachedCompressionBuilder::default()`] or [`CachedCompression::builder()`] to create a new builder,
/// configure it with the builder methods, and call [`build()`](CachedCompressionBuilder::build) to create
/// the final [`CachedCompression`] fairing.
#[derive(Debug, Clone)]
pub struct CachedCompressionBuilder {
    cached_paths: Vec<String>,
    cached_path_prefixes: Vec<String>,
    cached_path_suffixes: Vec<String>,
    excluded_path_prefixes: Vec<String>,
    level: Option<Level>,
    max_body_size: u64,
    compression_timeout: Duration,
    cache_max_capacity: u64,
    cache_ttl: Duration,
}

impl Default for CachedCompressionBuilder {
    fn default() -> Self {
        Self {
            cached_paths: Vec::new(),
            cached_path_prefixes: Vec::new(),
            cached_path_suffixes: Vec::new(),
            excluded_path_prefixes: Vec::new(),
            level: None,
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            compression_timeout: DEFAULT_COMPRESSION_TIMEOUT,
            cache_max_capacity: DEFAULT_CACHE_MAX_CAPACITY,
            cache_ttl: DEFAULT_CACHE_TTL,
        }
    }
}

impl CachedCompressionBuilder {
    /// Sets the maximum number of entries the cache can hold.
    ///
    /// When the cache exceeds this capacity, least-recently-used entries are evicted.
    /// Default: 1000 entries.
    pub fn cache_max_capacity(mut self, capacity: u64) -> Self {
        self.cache_max_capacity = capacity;
        self
    }

    /// Sets the time-to-live for cached entries.
    ///
    /// Entries are automatically removed after this duration.
    /// Default: 1 hour.
    pub fn cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Sets the compression level.
    pub fn level(mut self, level: Level) -> Self {
        self.level = Some(level);
        self
    }

    /// Sets the exact paths to cache.
    pub fn cached_paths(mut self, paths: Vec<String>) -> Self {
        self.cached_paths = paths;
        self
    }

    /// Sets the path prefixes to cache.
    pub fn cached_path_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.cached_path_prefixes = prefixes;
        self
    }

    /// Sets the path suffixes to cache.
    pub fn cached_path_suffixes(mut self, suffixes: Vec<String>) -> Self {
        self.cached_path_suffixes = suffixes;
        self
    }

    /// Sets the path prefixes to exclude from caching.
    pub fn excluded_path_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.excluded_path_prefixes = prefixes;
        self
    }

    /// Sets the maximum body size that will be compressed.
    ///
    /// Bodies larger than this size will not be compressed to prevent memory exhaustion.
    /// Default: 50 MiB (see [`DEFAULT_MAX_BODY_SIZE`]).
    pub fn max_body_size(mut self, size: u64) -> Self {
        self.max_body_size = size;
        self
    }

    /// Sets the compression timeout.
    ///
    /// Compression operations taking longer than this will be aborted.
    /// Default: 30 seconds (see [`DEFAULT_COMPRESSION_TIMEOUT`]).
    pub fn compression_timeout(mut self, timeout: Duration) -> Self {
        self.compression_timeout = timeout;
        self
    }

    /// Builds the [`CachedCompression`] fairing.
    ///
    /// This creates the cache with the configured capacity and TTL settings.
    pub fn build(self) -> CachedCompression {
        CachedCompression {
            cached_paths: self.cached_paths,
            cached_path_prefixes: self.cached_path_prefixes,
            cached_path_suffixes: self.cached_path_suffixes,
            excluded_path_prefixes: self.excluded_path_prefixes,
            level: self.level,
            max_body_size: self.max_body_size,
            compression_timeout: self.compression_timeout,
            cache: build_cache(self.cache_max_capacity, self.cache_ttl),
        }
    }
}

impl CachedCompression {
    /// Creates a new builder for `CachedCompression`.
    ///
    /// Default cache settings:
    /// - Maximum capacity: 1000 entries ([`DEFAULT_CACHE_MAX_CAPACITY`])
    /// - Time-to-live: 1 hour ([`DEFAULT_CACHE_TTL`])
    pub fn builder() -> CachedCompressionBuilder {
        CachedCompressionBuilder::default()
    }

    /// Converts `Vec<&str>` to `Vec<String>`.
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
            None => std::io::Error::other("ErrorBody already read"),
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

        let preferred = CompressionUtils::preferred_encoding(request);
        if preferred.is_none() {
            return;
        }

        if CompressionUtils::already_encoded(response) {
            return;
        }

        let content_type = response.content_type();
        if CompressionUtils::skip_encoding(&content_type, &EXCLUSIONS) {
            return;
        }

        // preferred is guaranteed to be Some at this point due to earlier check
        let encoding = preferred.unwrap();
        let desired_encoding = match encoding {
            Encoding::Zstd => CachedEncoding::Zstd,
            Encoding::Brotli => CachedEncoding::Brotli,
            Encoding::Gzip => CachedEncoding::Gzip,
            Encoding::Deflate => CachedEncoding::Deflate,
            _ => return,
        };

        let cache_key = (path.clone(), desired_encoding);

        if let Some(cached_body) = self.cache.get(&cache_key).await {
            debug!("Found cached response for {}", path);
            response.set_header(Header::new(CONTENT_ENCODING, format!("{}", encoding)));
            response.set_sized_body(cached_body.len(), Cursor::new(cached_body));
            return;
        }

        // Check body size before compression to prevent memory exhaustion
        if let Some(size) = response.body().preset_size() {
            if size > self.max_body_size as usize {
                warn!(
                    "Skipping compression for {}: body size {} exceeds max_body_size {}",
                    path, size, self.max_body_size
                );
                return;
            }
        }

        let body = response.body_mut().take();
        let compression_future = CompressionUtils::compress_body(
            body,
            desired_encoding,
            self.level.unwrap_or(Level::Default),
        );

        let compressed_body = match rocket::tokio::time::timeout(
            self.compression_timeout,
            compression_future,
        )
        .await
        {
            Ok(Ok(compressed_body)) => compressed_body,
            Ok(Err(err)) => {
                error!(
                    "Failed to compress response body for {}; underlying `AsyncRead` likely failed: {}",
                    path, err
                );
                response.set_streamed_body(ErrorBody(Some(err)));
                return;
            }
            Err(_) => {
                error!(
                    "Compression timeout for {}: exceeded {:?}",
                    path, self.compression_timeout
                );
                response.set_streamed_body(ErrorBody(Some(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "compression timeout",
                ))));
                return;
            }
        };

        response.set_header(Header::new(CONTENT_ENCODING, format!("{}", encoding)));

        // Check compressed size to prevent caching excessively large responses
        let should_cache = compressed_body.len() as u64 <= self.max_body_size;
        if !should_cache {
            warn!(
                "Skipping cache for {}: compressed size {} exceeds max_body_size {}",
                path,
                compressed_body.len(),
                self.max_body_size
            );
        }

        let len = compressed_body.len();
        if should_cache {
            debug!("Setting cached response for {}", path);
            self.cache.insert(cache_key, compressed_body.clone()).await;
        }
        response.set_sized_body(len, Cursor::new(compressed_body));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_compression_builder_cache_settings() {
        let builder = CachedCompression::builder()
            .cache_max_capacity(500)
            .cache_ttl(Duration::from_secs(1800))
            .max_body_size(10 * 1024 * 1024)
            .compression_timeout(Duration::from_secs(60));

        assert_eq!(builder.cache_max_capacity, 500);
        assert_eq!(builder.cache_ttl, Duration::from_secs(1800));
        assert_eq!(builder.max_body_size, 10 * 1024 * 1024);
        assert_eq!(builder.compression_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_cached_compression_builder() {
        let cc = CachedCompression::builder()
            .cached_paths(vec!["/".to_string(), "/about".to_string()])
            .cached_path_prefixes(vec!["/api/".to_string()])
            .cached_path_suffixes(vec![".js".to_string(), ".css".to_string()])
            .excluded_path_prefixes(vec!["/api/private/".to_string()])
            .level(Level::Fastest)
            .build();

        assert_eq!(cc.cached_paths, vec!["/", "/about"]);
        assert_eq!(cc.cached_path_prefixes, vec!["/api/"]);
        assert_eq!(cc.cached_path_suffixes, vec![".js", ".css"]);
        assert_eq!(cc.excluded_path_prefixes, vec!["/api/private/"]);
        assert!(cc.level.is_some());
    }

    #[test]
    fn test_static_paths_helper() {
        let paths = CachedCompression::static_paths(vec![".js", ".css", ".html"]);
        assert_eq!(
            paths,
            vec![".js".to_string(), ".css".to_string(), ".html".to_string()]
        );
    }
}
