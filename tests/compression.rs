use rocket::http::{Header, Status};
use rocket::local::blocking::Client;
use rocket::{Build, Rocket, get, routes};
use rocket_async_compression::CachedCompression;

// Use a longer, repetitive string that compresses well
const TEST_CONTENT: &str = "Hello, World! This is some text that should be compressed. \
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
    Lorem ipsum dolor sit amet, consectetur adipiscing elit.";

fn rocket_with_compression() -> Rocket<Build> {
    rocket::build().mount("/", routes![text_response]).attach(
        CachedCompression::builder()
            .cached_path_suffixes(vec![".txt".to_string()])
            .build(),
    )
}

#[get("/hello.txt")]
fn text_response() -> &'static str {
    TEST_CONTENT
}

#[test]
fn test_gzip_compression() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "gzip"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));

    let body = response.into_bytes().unwrap();
    assert!(
        body.len() < TEST_CONTENT.len(),
        "compressed size {} should be smaller than original {}",
        body.len(),
        TEST_CONTENT.len()
    );
}

#[test]
fn test_brotli_compression() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "br"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("br"));

    let body = response.into_bytes().unwrap();
    assert!(
        body.len() < TEST_CONTENT.len(),
        "compressed size {} should be smaller than original {}",
        body.len(),
        TEST_CONTENT.len()
    );
}

#[test]
fn test_deflate_compression() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "deflate"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Content-Encoding"),
        Some("deflate")
    );

    let body = response.into_bytes().unwrap();
    assert!(
        body.len() < TEST_CONTENT.len(),
        "compressed size {} should be smaller than original {}",
        body.len(),
        TEST_CONTENT.len()
    );
}

#[test]
fn test_zstd_compression() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "zstd"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("zstd"));

    let body = response.into_bytes().unwrap();
    assert!(
        body.len() < TEST_CONTENT.len(),
        "compressed size {} should be smaller than original {}",
        body.len(),
        TEST_CONTENT.len()
    );
}

#[test]
fn test_zstd_preferred_over_all() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "gzip, br, deflate, zstd"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    // Zstd should be preferred when all are accepted
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("zstd"));

    let body = response.into_bytes().unwrap();
    assert!(
        body.len() < TEST_CONTENT.len(),
        "compressed size {} should be smaller than original {}",
        body.len(),
        TEST_CONTENT.len()
    );
}

#[test]
fn test_brotli_preferred_over_gzip() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "gzip, br"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    // Brotli should be preferred when both are accepted
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("br"));

    let body = response.into_bytes().unwrap();
    assert!(
        body.len() < TEST_CONTENT.len(),
        "compressed size {} should be smaller than original {}",
        body.len(),
        TEST_CONTENT.len()
    );
}

#[test]
fn test_no_compression_without_accept_encoding() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    let response = client.get("/hello.txt").dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), None);

    let body = response.into_string().unwrap();
    assert_eq!(body, TEST_CONTENT);
}

// Q-value parsing tests

#[test]
fn test_qvalue_gzip_preferred() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    // gzip has higher q-value, should be preferred
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "br;q=0.5, gzip;q=1.0"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));

    let body = response.into_bytes().unwrap();
    assert!(body.len() < TEST_CONTENT.len());
}

#[test]
fn test_qvalue_zero_means_not_accepted() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    // br has q=0, should not be used even though it's normally preferred
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "br;q=0, gzip"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));
}

#[test]
fn test_qvalue_with_spaces_after_comma() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    // Test with spacing after commas (common in real headers)
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "gzip;q=0.8, br;q=0.5"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));
}

#[test]
fn test_identity_preferred_over_compression() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    // identity has higher q-value, no compression should be applied
    let response = client
        .get("/hello.txt")
        .header(Header::new(
            "Accept-Encoding",
            "gzip;q=0.5, br;q=0.5, identity;q=1.0",
        ))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), None);

    let body = response.into_string().unwrap();
    assert_eq!(body, TEST_CONTENT);
}

#[test]
fn test_compression_preferred_over_identity() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    // gzip has higher q-value than identity, should compress
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "gzip;q=1.0, identity;q=0.5"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));

    let body = response.into_bytes().unwrap();
    assert!(body.len() < TEST_CONTENT.len());
}

#[test]
fn test_no_compression_for_unsupported_encoding() {
    let client = Client::tracked(rocket_with_compression()).unwrap();
    // "compress" (LZW) is not a supported encoding
    let response = client
        .get("/hello.txt")
        .header(Header::new("Accept-Encoding", "compress"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), None);

    let body = response.into_string().unwrap();
    assert_eq!(body, TEST_CONTENT);
}
