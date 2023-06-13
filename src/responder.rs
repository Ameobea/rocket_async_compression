use rocket::response::{self, Responder, Response};
use rocket::Request;

use super::CompressionUtils;

/// Compresses responses with Brotli or Gzip compression using the `async-compression` crate.
///
/// The `Compress` type implements brotli and gzip compression for responses in
/// accordance with the `Accept-Encoding` header. If accepted, brotli
/// compression is preferred over gzip.
///
/// Responses that already have a `Content-Encoding` header are not compressed.
///
/// # Usage
///
/// Compress responses by wrapping a `Responder` inside `Compress`:
///
/// ```rust
/// use rocket_async_compression::Compress;
///
/// # #[allow(unused_variables)]
/// let response = Compress("Hi.");
/// ```
#[derive(Debug)]
pub struct Compress<R>(pub R, pub async_compression::Level);

impl<'r, 'o: 'r, R: Responder<'r, 'o>> Responder<'r, 'o> for Compress<R> {
    #[inline(always)]
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'o> {
        let mut response = Response::build()
            .merge(self.0.respond_to(request)?)
            .finalize();

        CompressionUtils::compress_response(request, &mut response, &[], self.1);
        Ok(response)
    }
}
