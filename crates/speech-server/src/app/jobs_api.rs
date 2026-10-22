use crate::{jobs, AppState};
use crate::jobs::store as job_store;
use axum::extract::Path as AxPath;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::sse::Sse;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use futures_util::stream::Stream;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;

#[cfg(test)]
mod tests;

/// Ответ на неудачную постановку джобы: 409 с id уже идущей джобы того же класса или 500.
pub(crate) fn enqueue_error(e: jobs::EnqueueError) -> Response {
    match e {
        jobs::EnqueueError::Conflict { job_id, kind } => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "job_conflict", "job_id": job_id, "kind": kind.as_str() })),
        )
            .into_response(),
        jobs::EnqueueError::Store(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("job.json: {e}")).into_response(),
    }
}

pub(super) async fn job_events(State(st): State<AppState>, AxPath(job_id): AxPath<String>) -> Response {
    let Some(sub) = st.jobs.subscribe(&job_id).await else {
        return (StatusCode::NOT_FOUND, "job not found").into_response();
    };
    Sse::new(sse_stream(sub))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// Активные и недавние джобы (без кадр-джоб). С `pid` — только этого проекта плюс его job.json
/// (последняя джоба проекта, в т.ч. прерванная до рестарта: её можно продолжить).
pub(super) async fn jobs_list(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let pid = q.get("pid").map(String::as_str).filter(|s| !s.is_empty());
    let project_job = match pid {
        Some(pid) => {
            let dir = match st.proj_dir(pid) {
                Ok(d) => d,
                Err(resp) => return resp,
            };
            match job_store::read(&dir) {
                Ok(rec) => serde_json::to_value(rec).unwrap_or(Value::Null),
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
            }
        }
        None => Value::Null,
    };
    Json(json!({ "jobs": st.jobs.list(pid).await, "project_job": project_job })).into_response()
}

/// Снапшот джобы {id, kind, pid, status, stage, msg, pct, position?, result|error}. `wait=N` (сек,
/// до jobs::WAIT_LONGEST_SECS) — long-poll: ответ приходит при завершении или по истечении N.
pub(super) async fn job_get(
    State(st): State<AppState>,
    AxPath(job_id): AxPath<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let wait: u64 = match q.get("wait").map(|v| v.parse::<u64>()) {
        None => 0,
        Some(Ok(v)) => v,
        Some(Err(_)) => return (StatusCode::BAD_REQUEST, t!("jobs-wait-not-number")).into_response(),
    };
    let snap = if wait > 0 {
        st.jobs.wait(&job_id, wait).await
    } else {
        st.jobs.snapshot(&job_id).await
    };
    match snap {
        Some(v) => Json(v).into_response(),
        None => (StatusCode::NOT_FOUND, "job not found").into_response(),
    }
}

pub(super) async fn job_cancel(State(st): State<AppState>, AxPath(job_id): AxPath<String>) -> Response {
    match st.jobs.cancel(&job_id).await {
        Ok(v) => Json(v).into_response(),
        Err(jobs::CancelError::NotFound) => (StatusCode::NOT_FOUND, "job not found").into_response(),
        Err(jobs::CancelError::Finished) => {
            (StatusCode::CONFLICT, Json(json!({ "error": "job_finished", "job_id": job_id }))).into_response()
        }
    }
}

/// Поток SSE-событий джобы: сначала терминал (если уже завершена) или последнее известное событие,
/// затем живые события до done/error/cancelled. Джоба из истории не удаляется — её может прочитать
/// следующий подписчик или GET /jobs/{id}.
pub(super) fn sse_stream(sub: jobs::Subscription) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let jobs::Subscription { first, terminal, mut rx } = sub;
        if let Some(ev) = first {
            yield sse_event(&ev);
            if terminal {
                return;
            }
        }
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let end = jobs::is_terminal_event(&ev);
                    yield sse_event(&ev);
                    if end {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break, // канал закрыт
            }
        }
    }
}

pub(super) fn sse_event(ev: &Value) -> Result<Event, Infallible> {
    Ok(Event::default().data(serde_json::to_string(ev).unwrap_or_default()))
}
