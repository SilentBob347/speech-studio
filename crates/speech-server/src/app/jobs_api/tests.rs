use super::*;
use axum::body::to_bytes;

async fn data(sub: jobs::Subscription) -> Vec<Value> {
    let response = Sse::new(sse_stream(sub)).into_response();
    assert_eq!(response.headers()[axum::http::header::CONTENT_TYPE], "text/event-stream");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[tokio::test]
async fn a_completed_subscription_replays_its_terminal_once() {
    let (tx, rx) = tokio::sync::broadcast::channel(4);
    tx.send(json!({"type":"progress"})).unwrap();
    let terminal = json!({"type":"done", "result":{"file":"a.wav"}});
    assert_eq!(
        data(jobs::Subscription {
            first: Some(terminal.clone()),
            terminal: true,
            rx
        })
        .await,
        [terminal]
    );
}

#[tokio::test]
async fn live_sse_keeps_the_snapshot_progress_and_first_terminal() {
    for kind in ["done", "error", "cancelled"] {
        let (tx, rx) = tokio::sync::broadcast::channel(8);
        let first = json!({"type":"running"});
        let progress = json!({"type":"progress", "pct":12});
        let end = json!({"type":kind});
        tx.send(progress.clone()).unwrap();
        tx.send(end.clone()).unwrap();
        tx.send(json!({"type":"after-terminal"})).unwrap();
        assert_eq!(
            data(jobs::Subscription {
                first: Some(first.clone()),
                terminal: false,
                rx
            })
            .await,
            [first, progress, end]
        );
    }
}

#[tokio::test]
async fn a_lagged_sse_subscription_still_delivers_the_terminal() {
    let (tx, rx) = tokio::sync::broadcast::channel(1);
    tx.send(json!({"type":"progress"})).unwrap();
    tx.send(json!({"type":"done"})).unwrap();
    assert_eq!(
        data(jobs::Subscription {
            first: None,
            terminal: false,
            rx
        })
        .await,
        [json!({"type":"done"})]
    );
}
