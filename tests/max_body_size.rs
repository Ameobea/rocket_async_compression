use rocket::http::{Header, Status};
use rocket::local::blocking::Client;
use rocket::{Build, Rocket, get, routes};
use rocket_async_compression::CachedCompression;

fn rocket_with_max_body_size(max_size: u64) -> Rocket<Build> {
    rocket::build()
        .mount("/", routes![small_response, large_response])
        .attach(
            CachedCompression::builder()
                .cached_path_suffixes(vec![".txt".to_string()])
                .max_body_size(max_size)
                .build(),
        )
}

#[get("/small.txt")]
fn small_response() -> &'static str {
    "Hello, World!"
}

#[get("/large.txt")]
fn large_response() -> String {
    "x".repeat(1000)
}

#[test]
fn test_small_body_is_compressed() {
    let client = Client::tracked(rocket_with_max_body_size(500)).unwrap();
    let response = client
        .get("/small.txt")
        .header(Header::new("Accept-Encoding", "gzip"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));
}

#[test]
fn test_large_body_skips_compression() {
    // Set max_body_size to 500 bytes, response is 1000 bytes
    let client = Client::tracked(rocket_with_max_body_size(500)).unwrap();
    let response = client
        .get("/large.txt")
        .header(Header::new("Accept-Encoding", "gzip"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    // Should not be compressed because body exceeds max_body_size
    assert_eq!(response.headers().get_one("Content-Encoding"), None);
    // Body should still be returned uncompressed
    assert_eq!(response.into_string().unwrap().len(), 1000);
}

#[test]
fn test_large_body_within_limit_is_compressed() {
    // Set max_body_size to 2000 bytes, response is 1000 bytes
    let client = Client::tracked(rocket_with_max_body_size(2000)).unwrap();
    let response = client
        .get("/large.txt")
        .header(Header::new("Accept-Encoding", "gzip"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.headers().get_one("Content-Encoding"), Some("gzip"));
}
