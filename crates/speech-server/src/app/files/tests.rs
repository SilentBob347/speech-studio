use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Request};

#[tokio::test]
async fn ranges_and_unicode_download_names_keep_their_headers_and_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("clip.wav");
    std::fs::write(&path, b"0123456789").unwrap();
    let request = Request::builder().header(header::RANGE, "bytes=2-5").body(Body::empty()).unwrap();
    let response = serve_file_range(&path, request, Some("Тест.wav".into())).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
    let disposition = response.headers()[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(disposition.contains("filename=\"____.wav\""), "{disposition}");
    assert!(
        disposition.contains("filename*=UTF-8''%D0%A2%D0%B5%D1%81%D1%82%2Ewav"),
        "{disposition}"
    );
    assert_eq!(&to_bytes(response.into_body(), usize::MAX).await.unwrap()[..], b"2345");
}

#[tokio::test]
async fn missing_files_and_unsatisfied_ranges_keep_http_errors() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("clip.wav");
    let response = serve_file_range(&path, Request::new(Body::empty()), None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    std::fs::write(&path, b"abc").unwrap();
    let request = Request::builder().header(header::RANGE, "bytes=99-").body(Body::empty()).unwrap();
    let response = serve_file_range(&path, request, None).await;
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */3");
}
