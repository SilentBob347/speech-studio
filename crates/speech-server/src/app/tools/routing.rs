//! In-process REST calls and streamed multipart uploads for application tools.

use super::*;

// ---------------------------------------------------------------- calling a route

/// The router the tools call.
pub(super) fn studio_api() -> Result<Router, String> {
    API.get().cloned().ok_or_else(|| "the studio is still starting".to_string())
}

/// A route's answer as it came.
pub(super) struct Reply {
    pub(super) status: StatusCode,
    pub(super) mime: String,
    pub(super) bytes: axum::body::Bytes,
}

/// Builds the route's request and calls it inside the process. A page instead of an answer means the route is not
/// there.
pub(super) async fn respond(call: Call) -> Result<Response, String> {
    let api = studio_api()?;
    let asked = format!("{} {}", call.method, call.path);
    let mut builder = Request::builder()
        .method(call.method)
        .uri(&call.path)
        .header(header::HOST, "127.0.0.1")
        .header(window::AGENT_HEADER, "1");
    // the OpenAI-compatible routes ask other programs for the key; a tool is the studio itself
    if call.path.starts_with("/v1/") {
        if let Some((token, _)) = crate::platform::credentials::api_token() {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
    }
    let request = match call.payload {
        Payload::None | Payload::Stream { body: None, .. } => builder.body(Body::empty()),
        Payload::Window { command, .. } => return Err(format!("{command} is a command of the studio's window, not a route")),
        Payload::Json(body) | Payload::Stream { body: Some(body), .. } => builder.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
        Payload::Form { fields, files } => {
            let boundary = format!("studio-mcp-{}", uuid::Uuid::new_v4().simple());
            let mut parts: Vec<Part> = Vec::new();
            for (name, value) in fields {
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")));
            }
            for (name, path, file) in files {
                if let Err(error) = tokio::fs::metadata(&path).await {
                    return Err(format!("read {}: {error}", path.display()));
                }
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", file.replace('"', "'"))));
                parts.push(Part::File(path));
                parts.push(Part::Text("\r\n".into()));
            }
            parts.push(Part::Text(format!("--{boundary}--\r\n")));
            builder.header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}")).body(Body::from_stream(streamed(parts)))
        }
    }
    .map_err(|error| error.to_string())?;
    let response = api.oneshot(request).await.map_err(|error| error.to_string())?;
    if mime_of(&response).starts_with("text/html") {
        return Err(format!("The studio has no route {asked}: its page answered instead."));
    }
    Ok(response)
}

pub(super) fn mime_of(response: &Response) -> String {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

/// A route's whole answer.
pub(super) async fn reply_of(response: Response) -> Result<Reply, String> {
    let status = response.status();
    let mime = mime_of(&response);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Reply { status, mime, bytes })
}

/// Calls a route inside the process and returns what it answered.
pub(super) async fn call_route_raw(call: Call) -> Result<Reply, String> {
    reply_of(respond(call).await?).await
}

/// A route's answer as text, cut to a size an agent reads.
pub(super) fn reply_text(reply: &Reply) -> String {
    let mut text = match serde_json::from_slice::<Value>(&reply.bytes) {
        Ok(value) => serde_json::to_string(&value).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(&reply.bytes).into_owned(),
    };
    if text.trim().is_empty() {
        text = if reply.status.is_success() {
            "Done.".into()
        } else {
            reply.status.to_string()
        };
    }
    text
}

/// Calls a route inside the process and returns what it answered.
pub(super) async fn call_route(call: Call) -> Result<(StatusCode, String), String> {
    let reply = call_route_raw(call).await?;
    Ok((reply.status, reply_text(&reply)))
}

/// A piece of a multipart body: text, or a file read as it is sent.
pub(super) enum Part {
    Text(String),
    File(PathBuf),
}

/// The parts as a stream, a file a megabyte at a time: a long video is sent
/// without ever being held in memory whole.
pub(super) fn streamed(parts: Vec<Part>) -> impl futures_util::Stream<Item = std::io::Result<axum::body::Bytes>> {
    futures_util::stream::unfold((parts.into_iter(), None::<tokio::fs::File>), |(mut parts, mut open)| async move {
        loop {
            if let Some(file) = open.as_mut() {
                let mut chunk = vec![0u8; 1 << 20];
                match tokio::io::AsyncReadExt::read(file, &mut chunk).await {
                    Ok(0) => open = None,
                    Ok(read) => {
                        chunk.truncate(read);
                        return Some((Ok(chunk.into()), (parts, open)));
                    }
                    Err(error) => return Some((Err(error), (parts, None))),
                }
                continue;
            }
            match parts.next()? {
                Part::Text(text) => return Some((Ok(text.into()), (parts, None))),
                Part::File(path) => match tokio::fs::File::open(&path).await {
                    Ok(file) => open = Some(file),
                    Err(error) => return Some((Err(error), (parts, None))),
                },
            }
        }
    })
}
