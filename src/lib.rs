//! Gzip and Brotli response compression.
//!
//! See the [`Compression`] and [`Compress`] types for further details.
//!
//! # Security Implications
//!
//! In some cases, HTTP compression on a site served over HTTPS can make a web
//! application vulnerable to attacks including BREACH. These risks should be
//! evaluated in the context of your application before enabling compression.
//!

#[macro_use]
extern crate log;

mod fairing;
mod responder;

pub use self::{
    fairing::{CachedCompression, Compression},
    responder::Compress,
};

use fairing::CachedEncoding;
use rocket::{
    http::{hyper::header::CONTENT_ENCODING, MediaType},
    response::Body,
    Request, Response,
};

pub enum Encoding {
    /// The `chunked` encoding.
    Chunked,
    /// The `br` encoding.
    Brotli,
    /// The `gzip` encoding.
    Gzip,
    /// The `deflate` encoding.
    Deflate,
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
            CONTENT_ENCODING.as_str(),
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

    /// Returns a tuple of the form (accepts_gzip, accepts_br).
    fn accepted_algorithms(request: &Request<'_>) -> (bool, bool) {
        request
            .headers()
            .get("Accept-Encoding")
            .flat_map(|accept| accept.split(','))
            .map(|accept| accept.trim())
            .fold((false, false), |(accepts_gzip, accepts_br), encoding| {
                (
                    accepts_gzip || encoding == "gzip",
                    accepts_br || encoding == "br",
                )
            })
    }

    async fn compress_body<'r>(
        body: Body<'r>,
        encoding: CachedEncoding,
        level: async_compression::Level
    ) -> std::io::Result<Vec<u8>> {
        match encoding {
            CachedEncoding::Brotli => {
                let mut compressor = async_compression::tokio::bufread::BrotliEncoder::with_quality(
                    rocket::tokio::io::BufReader::new(body),
                    level,
                );
                let mut out = Vec::new();
                rocket::tokio::io::copy(&mut compressor, &mut out).await?;
                Ok(out)
            }
            CachedEncoding::Gzip => {
                let mut compressor = async_compression::tokio::bufread::GzipEncoder::with_quality(
                    rocket::tokio::io::BufReader::new(body),
                    level,
                );
                let mut out = Vec::new();
                rocket::tokio::io::copy(&mut compressor, &mut out).await?;
                Ok(out)
            }
        }
    }

    fn compress_response<'r>(
        request: &Request<'_>,
        response: &'_ mut Response<'r>,
        exclusions: &[MediaType],
        level: async_compression::Level
    ) {
        if CompressionUtils::already_encoded(response) {
            return;
        }

        let content_type = response.content_type();

        if CompressionUtils::skip_encoding(&content_type, exclusions) {
            return;
        }

        let (accepts_gzip, accepts_br) = Self::accepted_algorithms(request);

        if !accepts_gzip && !accepts_br {
            return;
        }

        let body = response.body_mut().take();

        // Compression is done when the request accepts brotli or gzip encoding
        if accepts_br {
            let compressor = async_compression::tokio::bufread::BrotliEncoder::with_quality(
                rocket::tokio::io::BufReader::new(body),
                level,
            );

            CompressionUtils::set_body_and_encoding(response, compressor, Encoding::Brotli);
        } else if accepts_gzip {
            let compressor = async_compression::tokio::bufread::GzipEncoder::with_quality(
                rocket::tokio::io::BufReader::new(body),
                level,
            );

            CompressionUtils::set_body_and_encoding(response, compressor, Encoding::Gzip);
        }
    }
}
