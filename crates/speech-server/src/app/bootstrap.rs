use super::{augment_path_for_tools, build_router, init_proxy_route};
use crate::app::{self, hub, service};
use crate::listening::watching as listen_watch;
use crate::{setup, AppState};
use std::path::Path;

/// Поднять axum-сервер БЛОКИРУЮЩЕ на собственном tokio-рантайме. Для встраивания в десктоп-оболочку ОДНИМ
/// процессом (вместо запуска speech-server.exe отдельным subprocess) — вызывать из фонового std::thread.
/// Слушатель уже занят вызывающим (service::claim_port); augment PATH под инструменты делается здесь же.
pub fn serve_blocking(repo_root: impl AsRef<Path>, listener: std::net::TcpListener) -> anyhow::Result<()> {
    let root = repo_root.as_ref().to_path_buf();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        augment_path_for_tools(&root);
        init_proxy_route(&root);
        serve(AppState::new(&root), listener).await
    })
}

/// Обслуживать API и SPA на занятом слушателе до остановки рантайма.
pub async fn serve(state: AppState, listener: std::net::TcpListener) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    service::record_port(listener.local_addr()?.port());
    let listener = tokio::net::TcpListener::from_std(listener)?;
    hub::start(&state.repo_root, &state.workspace);
    listen_watch::resume(app::listening::watch_dependencies(&state));
    // models another program already has are linked in at start; only what is missing everywhere is downloaded
    let root = state.repo_root.clone();
    tokio::task::spawn_blocking(move || {
        for (id, app) in setup::import_from_neighbors(&root, &hub::neighbors(), false) {
            tracing::info!("model {id} taken from {app}, not downloaded");
        }
    });
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
