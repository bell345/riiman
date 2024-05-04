use std::path::{Path, PathBuf};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use eframe::egui;
use magick_rust::bindings::FilterType_LanczosFilter;
use magick_rust::MagickWand;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskError};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbnailParams {
    pub path: Box<Path>,
    pub last_modified: Option<DateTime<Utc>>,
    pub height: usize,
}

impl ThumbnailParams {
    pub fn tex_name(&self) -> String {
        format!("{}_{}", self.path.display(), self.height)
    }

    pub fn hash_path(&self) -> Box<Path> {
        let id = format!(
            "{}_{}_{}",
            self.path.display(),
            self.height,
            self.last_modified
                .map(|dt| dt.to_rfc3339())
                .unwrap_or("".to_string())
        );
        let h = base16ct::lower::encode_string(Sha256::digest(id).as_slice());
        // 6f12a101d9[...] -> riiman/6f/12a101d9[...].jpg
        let root_folder = "riiman";
        let folder = &h[..2];
        let file = &format!("{}.jpg", &h[2..]);
        let buf: PathBuf = [root_folder, folder, file].iter().collect();
        buf.into()
    }
}

async fn read_thumbnail(path: impl AsRef<Path>) -> Result<MagickWand, TaskError> {
    let wand = MagickWand::new();
    wand.read_image(path.as_ref().to_str().ok_or(AppError::InvalidUnicode)?)?;
    Ok(wand)
}

async fn read_and_resize(
    state: AppStateRef,
    params: &ThumbnailParams,
) -> Result<MagickWand, TaskError> {
    let abs_path = {
        let state = state.read().await;
        let vault = state.get_current_vault().ok_or(AppError::NoCurrentVault)?;
        vault.resolve_abs_path(&params.path)?
    };

    let wand = MagickWand::new();
    wand.read_image(&abs_path)?;

    let orig_height = wand.get_image_height();
    let orig_width = wand.get_image_width();

    let new_height = params.height;
    let new_width = (orig_width as f32 / orig_height as f32 * new_height as f32).floor() as usize;

    wand.resize_image(new_width, new_height, FilterType_LanczosFilter);

    Ok(wand)
}

fn export_all_rgba(wand: &MagickWand) -> anyhow::Result<Vec<u8>> {
    wand.export_image_pixels(
        0,
        0,
        wand.get_image_width(),
        wand.get_image_height(),
        "RGBA",
    )
    .ok_or_else(|| match wand.get_exception() {
        Ok((e, _)) => anyhow!("{e:?}"),
        Err(e) => e.into(),
    })
}

pub async fn load_image_thumbnail_with_fs(
    state: AppStateRef,
    progress: ProgressSenderRef,
    params: ThumbnailParams,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(TaskError::WasmNotImplemented);
    }

    progress.send(ProgressState::Indeterminate);

    let hash_file = std::env::temp_dir().join(params.hash_path());
    let hash_file_str = hash_file.to_str().ok_or(AppError::InvalidUnicode)?;
    tokio::fs::create_dir_all(hash_file.parent().unwrap()).await?;

    let wand = match tokio::fs::metadata(&hash_file).await {
        Ok(meta) if meta.is_file() => read_thumbnail(&hash_file).await?,
        _ => read_and_resize(state, &params).await?,
    };

    wand.write_image(hash_file_str)?;

    let rgba = export_all_rgba(&wand)?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [wand.get_image_width(), wand.get_image_height()],
        &rgba,
    );

    Ok(AsyncTaskResult::ThumbnailLoaded { params, image })
}

pub async fn load_image_thumbnail(
    state: AppStateRef,
    progress: ProgressSenderRef,
    params: ThumbnailParams,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(TaskError::WasmNotImplemented);
    }

    progress.send(ProgressState::Indeterminate);

    let wand = read_and_resize(state, &params).await?;

    let rgba = export_all_rgba(&wand)?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [wand.get_image_width(), wand.get_image_height()],
        &rgba,
    );

    Ok(AsyncTaskResult::ThumbnailLoaded { params, image })
}
