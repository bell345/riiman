use crate::errors::AppError;
use crate::state::AppStateRef;
use anyhow::anyhow;
use eframe::egui;
use magick_rust::bindings::FilterType_LanczosFilter;
use magick_rust::{MagickError, MagickWand};
use std::path::Path;

use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbnailParams {
    pub path: Box<Path>,
    pub height: usize,
}

impl ThumbnailParams {
    pub fn tex_name(&self) -> String {
        format!("{}_{}", self.path.display(), self.height)
    }
}

fn unsharp_image(wand: &MagickWand) -> Result<(), MagickError> {
    let res =
        unsafe { magick_rust::bindings::MagickUnsharpMaskImage(wand.wand, 10.0, 4.0, 1.0, 0.0) };
    match res {
        magick_rust::bindings::MagickBooleanType_MagickTrue => Ok(()),
        _ => Err(MagickError("failed to unsharp image thumbnail")),
    }
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

pub async fn load_image_thumbnail(
    state: AppStateRef,
    progress: ProgressSenderRef,
    params: ThumbnailParams,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(TaskError::WasmNotImplemented);
    }

    let abs_path = {
        let state = state.read().await;
        let vault = state.get_current_vault().ok_or(AppError::NoCurrentVault)?;
        vault.resolve_abs_path(&params.path)?
    };

    progress.send(ProgressState::Indeterminate);

    let wand = MagickWand::new();
    wand.read_image(&abs_path)?;

    let orig_height = wand.get_image_height();
    let orig_width = wand.get_image_width();

    let new_height = params.height;
    let new_width = (orig_width as f32 / orig_height as f32 * new_height as f32).floor() as usize;

    wand.resize_image(new_width, new_height, FilterType_LanczosFilter);
    //unsharp_image(&wand)?;

    let rgba = export_all_rgba(&wand)?;
    let image = egui::ColorImage::from_rgba_unmultiplied([new_width, new_height], &rgba);

    Ok(AsyncTaskResult::ThumbnailLoaded { params, image })
}
