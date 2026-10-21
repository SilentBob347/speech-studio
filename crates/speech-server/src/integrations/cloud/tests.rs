use super::*;
use base64::Engine;
use dub_llm::test_http::{serve, Reply};

#[test]
fn the_client_decodes_the_first_audio_output_and_reports_invalid_data() {
    let client = Client::new().unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("audio.wav");
    std::fs::write(&path, b"file bytes").unwrap();
    assert_eq!(
        client
            .audio(&[
                Output::Text { text: "ignore".into() },
                Output::File {
                    path,
                    mime: "audio/wav".into()
                }
            ])
            .unwrap(),
        b"file bytes"
    );
    let outputs = [Output::Data {
        data_base64: base64::engine::general_purpose::STANDARD.encode(b"data"),
        mime: "audio/wav".into(),
    }];
    assert_eq!(client.audio(&outputs).unwrap(), b"data");
    assert!(client
        .audio(&[Output::Data {
            data_base64: "invalid!".into(),
            mime: "audio/wav".into()
        }])
        .is_err());
}

#[test]
fn the_client_downloads_provider_output_bytes_without_project_or_queue_state() {
    let server = serve(vec![Reply::json(200, "provider audio")]);
    let client = Client::new().unwrap();
    assert_eq!(
        client
            .audio(&[Output::Url {
                url: format!("{}/audio", server.base()),
                mime: None
            }])
            .unwrap(),
        b"provider audio"
    );
    assert_eq!(server.count(), 1);
    assert!(server.request(0).starts_with("GET /audio"));
}
