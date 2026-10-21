//! Requests and protocol conversion for fal and Runware. Callers supply credentials and operation state.

use serde_json::{Map, Value};
use std::time::Duration;
use vs_providers::{fal, runware, ModelEntry, ModelForm, Output, Polled, Provider, ProviderError, Role, Submitted};

pub(crate) struct Client {
    http: vs_providers::http::Http,
}

#[cfg(test)]
mod tests;

pub(crate) fn block<F: std::future::Future>(work: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(work)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for a provider call")
            .block_on(work),
    }
}

pub(crate) fn field_key(form: &ModelForm, role: Role, names: &[&str]) -> Option<String> {
    form.fields
        .iter()
        .find(|f| f.role == Some(role))
        .or_else(|| form.fields.iter().find(|f| names.contains(&f.key.as_str())))
        .map(|f| f.key.clone())
}

impl Client {
    pub(crate) fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            http: vs_providers::http::Http::new(reqwest::Client::builder().timeout(Duration::from_secs(120)))?,
        })
    }

    pub(crate) async fn catalog(&self, provider: Provider) -> Result<Vec<ModelEntry>, String> {
        match provider {
            Provider::Runware => runware::catalog(&self.http).await.map_err(|e| e.to_string()),
            Provider::Fal => {
                let (entries, failures) = fal::catalog(&self.http, 3).await;
                if entries.is_empty() {
                    Err(failures.join("; "))
                } else {
                    Ok(entries)
                }
            }
            _ => unreachable!("only catalog providers run here"),
        }
    }

    pub(crate) async fn schema(&self, url: &str) -> Result<Value, String> {
        self.http.get_json(url, None).await.map_err(|e| e.to_string())
    }

    pub(crate) async fn fal_form(&self, id: &str) -> Result<ModelForm, String> {
        let document = fal::openapi(&self.http, id).await.map_err(|e| e.to_string())?;
        fal::form(id, &document).map_err(|e| e.to_string())
    }

    pub(crate) async fn fal_price(&self, key: &str, id: &str) -> Result<vs_providers::PriceRule, String> {
        fal::price(&self.http, key, id).await.map_err(|e| e.to_string())
    }

    pub(crate) fn runware_form(&self, id: &str, schema: &Value) -> Result<ModelForm, String> {
        runware::form(id, schema).map_err(|e| e.to_string())
    }

    pub(crate) async fn balance(&self, provider: Provider, key: &str) -> Result<Option<f64>, ProviderError> {
        match provider {
            Provider::Fal => fal::balance(&self.http, key).await,
            _ => runware::balance(&self.http, key).await,
        }
    }

    pub(crate) async fn submit(
        &self,
        provider: Provider,
        key: &str,
        id: &str,
        form: &ModelForm,
        values: &Map<String, Value>,
    ) -> Result<Submitted, ProviderError> {
        match provider {
            Provider::Fal => fal::submit(&self.http, key, id, fal::build_input(values)).await,
            Provider::Runware => {
                runware::submit(
                    &self.http,
                    key,
                    runware::build_task(id, form, values, &uuid::Uuid::new_v4().to_string()),
                )
                .await
            }
            _ => unreachable!("only catalog providers run here"),
        }
    }

    pub(crate) async fn poll(&self, provider: Provider, key: &str, handle: &Value) -> Result<Polled, ProviderError> {
        match provider {
            Provider::Fal => fal::poll(&self.http, key, handle).await,
            _ => runware::poll(&self.http, key, handle).await,
        }
    }

    pub(crate) async fn upload(&self, key: &str, payload: &vs_providers::MediaPayload) -> Result<String, ProviderError> {
        fal::upload(&self.http, key, payload).await
    }

    pub(crate) fn audio(&self, outputs: &[Output]) -> Result<Vec<u8>, String> {
        use base64::Engine;
        for output in outputs {
            match output {
                Output::Data { data_base64, .. } => {
                    return base64::engine::general_purpose::STANDARD
                        .decode(data_base64)
                        .map_err(|e| e.to_string())
                }
                Output::Url { url, .. } => return block(self.http.get_bytes(url, None)).map(|(b, _)| b).map_err(|e| e.to_string()),
                Output::File { path, .. } => return std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())),
                Output::Text { .. } => {}
            }
        }
        Err(t!("cloud-no-audio"))
    }
}
