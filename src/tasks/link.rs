use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::{DateTime, NaiveDateTime, Utc};
use itertools::Itertools;

use crate::data::{FieldStore, FieldValue};
use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;
use crate::tasks::import::{on_import_result_send_progress, process_many, scan_recursively};
use crate::tasks::vault::save_vault;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, SingleImportResult};

const CONCURRENT_TASKS_LIMIT: usize = 16;

async fn link_single_sidecar(
    state: AppStateRef,
    path: PathBuf,
    sidecar_path: PathBuf,
    sidecar_date: DateTime<Utc>,
) -> SingleImportResult {
    let sc_path_string = sidecar_path.to_string_lossy().to_string();
    let item = state
        .read()
        .await
        .current_vault()?
        .get_item_opt(&path)?
        .ok_or(anyhow!(AppError::MissingItem {
            path: path.to_string_lossy().into()
        }))?
        .clone();

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

    if let Some(tweet_id) = dom.get("tweet_id").and_then(|tid| tid.as_u64()) {
        item.set_known_field_value(fields::tweet::ID, tweet_id);
    }

    if let Some(content) = dom.get("content").and_then(|c| c.as_str()) {
        item.set_known_field_value(fields::tweet::CONTENT, content.to_string().into());
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

    if let Some(author_id) = author.and_then(|a| a.get("id")).and_then(|id| id.as_u64()) {
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

    {
        let r = state.read().await;
        let vault = r.current_vault()?;
        let link_ref = item.link_ref()?;

        vault.update_item(&path, item)?;
        if let Some((other_vault_name, _)) = link_ref {
            let other_vault = r.get_vault(&other_vault_name)?;
            vault.update_link(&path, &other_vault)?;
        }
    }

    Ok(path.into_boxed_path())
}

pub async fn link_sidecars(state: AppStateRef, progress: ProgressSenderRef) -> AsyncTaskReturn {
    let root_dir = state.read().await.current_vault()?.root_dir()?;

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
        |(path, sc, sc_date)| link_single_sidecar(state.clone(), path, sc, sc_date),
        on_import_result_send_progress,
        CONCURRENT_TASKS_LIMIT,
    )
    .await?;

    {
        let r = state.read().await;
        let curr_vault = r.current_vault()?;
        save_vault(curr_vault, progress.sub_task("Save", 0.05)).await?;
    }

    Ok(AsyncTaskResult::None)
}

async fn link_single_item(
    state: AppStateRef,
    other_vault_name: Arc<String>,
    path: PathBuf,
) -> SingleImportResult {
    let r = state.read().await;
    let vault = r.current_vault()?;
    let other_vault = r.get_vault(&other_vault_name)?;

    {
        let item = vault.get_item(&path)?;
        let other_item = other_vault.get_item(&path)?;
        item.set_known_field_value(
            fields::general::LINK,
            (
                other_vault_name.to_string().into(),
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
    }
    vault.update_link(&path, &other_vault)?;

    Ok(path.into_boxed_path())
}

pub async fn link_vaults_by_path(
    other_vault_name: String,
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let other_name_arc = Arc::new(other_vault_name.clone());

    let paths: Vec<PathBuf> = state
        .read()
        .await
        .current_vault()?
        .iter_items()
        .map(|i| i.path().into())
        .collect();

    let results = process_many(
        paths,
        progress,
        |path| link_single_item(state.clone(), other_name_arc.clone(), path),
        on_import_result_send_progress,
        4,
    )
    .await?;

    Ok(AsyncTaskResult::LinkComplete {
        other_vault_name,
        results,
    })
}
