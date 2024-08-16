use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::{DateTime, NaiveDateTime, Utc};
use itertools::Itertools;
use tokio::task::spawn_blocking;

use crate::data::{FieldStore, FieldValue, Vault};
use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;
use crate::tasks::import::{on_import_result_send_progress, process_many, scan_recursively};
use crate::tasks::vault::{save_current_and_linked_vaults, save_vault};
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, SingleImportResult};

const CONCURRENT_TASKS_LIMIT: usize = 16;

async fn link_single_sidecar(
    state: AppStateRef,
    path: PathBuf,
    sidecar_path: PathBuf,
    sidecar_date: DateTime<Utc>,
    skip_save: bool,
) -> SingleImportResult {
    let sc_path_string = sidecar_path.to_string_lossy().to_string();
    let vault = state.current_vault()?;
    let item = vault.get_item(&path)?;

    if let Some(item_updated) = item.get_known_field_value(fields::general::SIDECAR_LAST_UPDATED)? {
        if sidecar_date <= item_updated {
            return Ok(path.into_boxed_path());
        }
    }

    let sidecar =
        serde_json::from_slice::<serde_json::Value>(&tokio::fs::read(sidecar_path).await?)
            .map_err(|e| {
                anyhow!(AppError::UnexpectedJsonSidecar {
                    path: sc_path_string.clone(),
                    error: Some(e.to_string())
                })
            })?;

    let dom = sidecar
        .as_object()
        .ok_or(anyhow!(AppError::UnexpectedJsonSidecar {
            path: sc_path_string,
            error: Some("not an object".into())
        }))?;

    if let Some(tweet_id) = dom.get("tweet_id").and_then(|tid| tid.as_i64()) {
        item.set_known_field_value(fields::tweet::ID, tweet_id);
    }

    if let Some(content) = dom.get("content").and_then(|c| c.as_str()) {
        item.set_known_field_value(fields::tweet::CONTENT, content.to_string().into());
    }

    if let Some(num) = dom.get("num").and_then(|c| c.as_i64()) {
        item.set_known_field_value(fields::tweet::IMAGE_NUMBER, num);
    }

    if let Some(hashtags) = dom.get("hashtags").and_then(|c| c.as_array()).map(|a| {
        a.iter()
            .filter_map(|ht| {
                ht.as_str()
                    .map(|s| FieldValue::string(s.to_string().into()))
            })
            .collect_vec()
    }) {
        item.set_known_field_value(fields::tweet::HASHTAGS, hashtags);
    }

    let author = dom.get("author").and_then(|a| a.as_object());

    if let Some(author_id) = author.and_then(|a| a.get("id")).and_then(|id| id.as_i64()) {
        item.set_known_field_value(fields::tweet::AUTHOR_ID, author_id);
    }

    if let Some(author_handle) = author.and_then(|a| a.get("name")).and_then(|n| n.as_str()) {
        item.set_known_field_value(
            fields::tweet::AUTHOR_HANDLE,
            author_handle.to_string().into(),
        );
    }

    if let Some(author_name) = author.and_then(|a| a.get("nick")).and_then(|n| n.as_str()) {
        item.set_known_field_value(fields::tweet::AUTHOR_NAME, author_name.to_string().into());
    }

    if let Some(tweet_date) = dom
        .get("date")
        .and_then(|d| d.as_str())
        .and_then(|d| NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M:%S").ok())
        .map(|d| d.and_utc())
    {
        item.set_known_field_value(fields::tweet::POST_DATE, tweet_date);
    }

    if let Some(liked_date) = dom
        .get("date_liked")
        .and_then(|d| d.as_str())
        .and_then(|d| NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M:%S").ok())
        .map(|d| d.and_utc())
    {
        item.set_known_field_value(fields::tweet::LIKED_DATE, liked_date);
    }

    // make sure to skip saving as it should only happen once afterwards
    state.commit_item(vault, &item, skip_save)?;

    Ok(path.into_boxed_path())
}

pub async fn link_sidecars(state: AppStateRef, progress: ProgressSenderRef) -> AsyncTaskReturn {
    let root_dir = state.current_vault()?.root_dir()?;

    let entries = scan_recursively(
        root_dir.as_path(),
        progress.sub_task("Scan", 0.05),
        |item, metadata| {
            Some((
                item.path().clone(),
                metadata
                    .modified()
                    .map(|m| -> DateTime<Utc> { m.into() })
                    .unwrap_or(Utc::now()),
            ))
        },
    )
    .await?;

    let json_ext = OsStr::new("json");

    let mut path_to_last_modified_map: HashMap<PathBuf, DateTime<Utc>> = entries
        .iter()
        .map(|(path, last_modified)| (path.clone(), *last_modified))
        .collect();

    let entries_with_sidecars = entries
        .into_iter()
        .filter_map(|(path, _date)| {
            if path.extension() == Some(json_ext) {
                return None;
            }

            let extension = match path.extension() {
                None => OsString::from("json"),
                Some(ext) => format!("{}.json", ext.to_str()?).into(),
            };

            if let Some((sidecar_path, sidecar_date)) =
                path_to_last_modified_map.remove_entry(&path.with_extension(extension))
            {
                Some((path, sidecar_path, sidecar_date))
            } else {
                None
            }
        })
        .collect_vec();

    process_many(
        entries_with_sidecars,
        progress.sub_task("Import", 0.90),
        |(path, sc, sc_date)| link_single_sidecar(state.clone(), path, sc, sc_date, true),
        on_import_result_send_progress,
        CONCURRENT_TASKS_LIMIT,
    )
    .await?;

    save_current_and_linked_vaults(state, progress.sub_task("Save vault", 0.05)).await
}

#[allow(clippy::needless_pass_by_value)]
fn link_single_item(
    vault: Arc<Vault>,
    other_vault: Arc<Vault>,
    state: AppStateRef,
    path: PathBuf,
    skip_save: bool,
) -> SingleImportResult {
    let item = vault.get_item(&path)?;
    let other_item = other_vault.get_item(&path)?;

    item.set_known_field_value(
        fields::general::LINK,
        (
            other_vault.name.to_string().into(),
            other_item.path_string().clone(),
        ),
    );
    other_item.set_known_field_value(
        fields::general::LINK,
        (
            vault.name.to_string().into(),
            path.to_string_lossy().to_string().into(),
        ),
    );

    state.commit_item(vault, &item, skip_save)?;

    Ok(path.into_boxed_path())
}

async fn link_single_item_task(
    vault: Arc<Vault>,
    other_vault: Arc<Vault>,
    state: AppStateRef,
    path: PathBuf,
    skip_save: bool,
) -> SingleImportResult {
    spawn_blocking(move || link_single_item(vault, other_vault, state, path, skip_save)).await?
}

pub async fn link_vaults_by_path(
    other_vault_name: String,
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let vault = state.current_vault()?;
    let other_vault = state.get_vault(&other_vault_name)?;

    let paths: Vec<PathBuf> = vault.iter_items().map(|i| i.path().into()).collect();

    let results = process_many(
        paths,
        progress,
        |path| {
            link_single_item_task(
                Arc::clone(&vault),
                Arc::clone(&other_vault),
                state.clone(),
                path,
                true,
            )
        },
        on_import_result_send_progress,
        4,
    )
    .await?;

    state.save_vault_deferred(vault);
    state.save_vault_deferred(other_vault);

    Ok(AsyncTaskResult::LinkComplete {
        other_vault_name,
        results,
    })
}
