//! Response compression for Rocket supporting Gzip, Brotli, Deflate, and Zstd
//!
//! See the [`Compression`] and [`Compress`] types for further details.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use rocket::{routes, launch};
//!
//! use rocket_async_compression::Compression;
//!
//! #[launch]
//! async fn rocket() -> _ {
//!     let server = rocket::build()
//!         .mount("/", routes![...]);
//!
//!     if cfg!(debug_assertions) {
//!         server
//!     } else {
//!         server.attach(Compression::fairing())
//!     }
//! }
//! ```
//!
//! ## Security Implications
//!
//! In some cases, HTTP compression on a site served over HTTPS can make a web
//! application vulnerable to attacks including BREACH. These risks should be
//! evaluated in the context of your application before enabling compression.

mod fairing;
mod responder;

pub use self::{
    fairing::{
        CachedCompression, CachedCompressionBuilder, Compression, DEFAULT_CACHE_MAX_CAPACITY,
        DEFAULT_CACHE_TTL, DEFAULT_COMPRESSION_TIMEOUT, DEFAULT_MAX_BODY_SIZE,
    },
    responder::Compress,
};

pub use async_compression::Level;
use fairing::CachedEncoding;
use http::header::{ACCEPT_ENCODING, HeaderMap, HeaderValue};
use rocket::{Request, Response, http::MediaType, response::Body};

const CONTENT_ENCODING: &str = "content-encoding";

#[derive(Clone)]
pub enum Encoding {
    /// The `chunked` encoding.
    Chunked,
    /// The `br` encoding.
    Brotli,
    /// The `gzip` encoding.
    Gzip,
    /// The `deflate` encoding.
    Deflate,
    /// The `zstd` encoding.
    Zstd,
    /// The `compress` encoding.
    Compress,
    /// The `identity` encoding.
    Identity,
    /// The `trailers` encoding.
    Trailers,
    /// Some other encoding that is less common, can be any String.
    EncodingExt(String),
}

impl std::fmt::Display for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Encoding::Chunked => "chunked",
            Encoding::Brotli => "br",
            Encoding::Gzip => "gzip",
            Encoding::Deflate => "deflate",
            Encoding::Zstd => "zstd",
            Encoding::Compress => "compress",
            Encoding::Identity => "identity",
            Encoding::Trailers => "trailers",
            Encoding::EncodingExt(ref s) => s.as_ref(),
        })
    }
}

impl std::str::FromStr for Encoding {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Encoding, std::convert::Infallible> {
        match s {
            "chunked" => Ok(Encoding::Chunked),
            "br" => Ok(Encoding::Brotli),
            "deflate" => Ok(Encoding::Deflate),
            "gzip" => Ok(Encoding::Gzip),
            "zstd" => Ok(Encoding::Zstd),
            "compress" => Ok(Encoding::Compress),
            "identity" => Ok(Encoding::Identity),
            "trailers" => Ok(Encoding::Trailers),
            _ => Ok(Encoding::EncodingExt(s.to_owned())),
        }
    }
}

struct CompressionUtils;

impl CompressionUtils {
    fn already_encoded(response: &Response<'_>) -> bool {
        response.headers().get("Content-Encoding").next().is_some()
    }

    fn set_body_and_encoding<'r, B: rocket::tokio::io::AsyncRead + Send + 'r>(
        response: &'_ mut Response<'r>,
        body: B,
        encoding: Encoding,
    ) {
        response.set_header(::rocket::http::Header::new(
            CONTENT_ENCODING,
            format!("{}", encoding),
        ));
        response.set_streamed_body(body);
    }

    fn skip_encoding(
        content_type: &Option<rocket::http::ContentType>,
        exclusions: &[MediaType],
    ) -> bool {
        match content_type {
            Some(content_type) => exclusions.iter().any(|exc_media_type| {
                if exc_media_type.sub() == "*" {
                    *exc_media_type.top() == *content_type.top()
                } else {
                    *exc_media_type == *content_type.media_type()
                }
            }),
            None => false,
        }
    }

    /// Builds an http::HeaderMap from Rocket's request headers for use with fly-accept-encoding.
    fn build_header_map(request: &Request<'_>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in request.headers().get("Accept-Encoding") {
            if let Ok(header_value) = HeaderValue::from_str(value) {
                headers.append(ACCEPT_ENCODING, header_value);
            }
        }
        headers
    }

    /// Returns the preferred encoding based on q-values using fly-accept-encoding.
    /// Returns None if identity is preferred or no compression is accepted.
    /// Priority when q-values are equal: zstd > brotli > gzip > deflate (by compression ratio).
    fn preferred_encoding(request: &Request<'_>) -> Option<Encoding> {
        let headers = Self::build_header_map(request);

        let mut gzip_q: Option<f32> = None;
        let mut br_q: Option<f32> = None;
        let mut deflate_q: Option<f32> = None;
        let mut zstd_q: Option<f32> = None;
        let mut identity_q: Option<f32> = None;

        for result in fly_accept_encoding::encodings_iter(&headers) {
            if let Ok((Some(encoding), q)) = result {
                match encoding {
                    fly_accept_encoding::Encoding::Gzip => {
                        gzip_q = Some(gzip_q.map_or(q, |existing| existing.max(q)));
                    }
                    fly_accept_encoding::Encoding::Brotli => {
                        br_q = Some(br_q.map_or(q, |existing| existing.max(q)));
                    }
                    fly_accept_encoding::Encoding::Deflate => {
                        deflate_q = Some(deflate_q.map_or(q, |existing| existing.max(q)));
                    }
                    fly_accept_encoding::Encoding::Zstd => {
                        zstd_q = Some(zstd_q.map_or(q, |existing| existing.max(q)));
                    }
                    fly_accept_encoding::Encoding::Identity => {
                        identity_q = Some(identity_q.map_or(q, |existing| existing.max(q)));
                    }
                }
            }
        }

        // Collect all supported encodings with their q-values
        // Priority when equal: zstd > brotli > gzip > deflate
        let mut candidates: Vec<(Encoding, f32, u8)> = Vec::new();
        if let Some(q) = zstd_q {
            candidates.push((Encoding::Zstd, q, 0)); // highest priority
        }
        if let Some(q) = br_q {
            candidates.push((Encoding::Brotli, q, 1));
        }
        if let Some(q) = gzip_q {
            candidates.push((Encoding::Gzip, q, 2));
        }
        if let Some(q) = deflate_q {
            candidates.push((Encoding::Deflate, q, 3)); // lowest priority
        }

        // Sort by q-value descending, then by priority ascending
        candidates.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });

        let best_compression = candidates.first().map(|(enc, q, _)| (enc.clone(), *q));

        // If identity is preferred over compression, return None
        if let Some(identity) = identity_q {
            match best_compression {
                Some((enc, q)) if q > identity => Some(enc),
                Some((enc, q)) if q == identity => Some(enc), // Prefer compression when equal
                _ => None,
            }
        } else {
            best_compression.map(|(enc, _)| enc)
        }
    }

    async fn compress_body<'r>(
        body: Body<'r>,
        encoding: CachedEncoding,
        level: async_compression::Level,
    ) -> std::io::Result<bytes::Bytes> {
        use rocket::tokio::io::AsyncReadExt;

        // Adjust brotli default level to 4 (matching Nginx) since the library default of 11 is too slow
        let level = if matches!(encoding, CachedEncoding::Brotli)
            && matches!(level, async_compression::Level::Default)
        {
            async_compression::Level::Precise(4)
        } else {
            level
        };

        let mut out = Vec::new();
        let reader = rocket::tokio::io::BufReader::new(body);

        match encoding {
            CachedEncoding::Zstd => {
                let mut compressor =
                    async_compression::tokio::bufread::ZstdEncoder::with_quality(reader, level);
                compressor.read_to_end(&mut out).await?;
            }
            CachedEncoding::Brotli => {
                let mut compressor =
                    async_compression::tokio::bufread::BrotliEncoder::with_quality(reader, level);
                compressor.read_to_end(&mut out).await?;
            }
            CachedEncoding::Gzip => {
                let mut compressor =
                    async_compression::tokio::bufread::GzipEncoder::with_quality(reader, level);
                compressor.read_to_end(&mut out).await?;
            }
            CachedEncoding::Deflate => {
                let mut compressor =
                    async_compression::tokio::bufread::DeflateEncoder::with_quality(reader, level);
                compressor.read_to_end(&mut out).await?;
            }
        }

        Ok(out.into())
    }

    fn compress_response<'r>(
        request: &Request<'_>,
        response: &'_ mut Response<'r>,
        exclusions: &[MediaType],
        level: async_compression::Level,
    ) {
        if CompressionUtils::already_encoded(response) {
            return;
        }

        let content_type = response.content_type();

        if CompressionUtils::skip_encoding(&content_type, exclusions) {
            return;
        }

        let encoding = match Self::preferred_encoding(request) {
            Some(enc) => enc,
            None => return,
        };

        let body = response.body_mut().take();

        match encoding {
            Encoding::Zstd => {
                let compressor = async_compression::tokio::bufread::ZstdEncoder::with_quality(
                    rocket::tokio::io::BufReader::new(body),
                    level,
                );
                CompressionUtils::set_body_and_encoding(response, compressor, Encoding::Zstd);
            }
            Encoding::Brotli => {
                let compressor = async_compression::tokio::bufread::BrotliEncoder::with_quality(
                    rocket::tokio::io::BufReader::new(body),
                    level,
                );
                CompressionUtils::set_body_and_encoding(response, compressor, Encoding::Brotli);
            }
            Encoding::Gzip => {
                let compressor = async_compression::tokio::bufread::GzipEncoder::with_quality(
                    rocket::tokio::io::BufReader::new(body),
                    level,
                );
                CompressionUtils::set_body_and_encoding(response, compressor, Encoding::Gzip);
            }
            Encoding::Deflate => {
                let compressor = async_compression::tokio::bufread::DeflateEncoder::with_quality(
                    rocket::tokio::io::BufReader::new(body),
                    level,
                );
                CompressionUtils::set_body_and_encoding(response, compressor, Encoding::Deflate);
            }
            // These encodings are not compression algorithms we support
            Encoding::Chunked
            | Encoding::Compress
            | Encoding::Identity
            | Encoding::Trailers
            | Encoding::EncodingExt(_) => {}
        }
    }
}
