use crate::errors::AppError;
use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use eframe::egui;
use eframe::egui::{vec2, Vec2};
use magick_rust::{FilterType, MagickWand};
use std::path::Path;

pub fn read_image(path: impl AsRef<Path>) -> anyhow::Result<MagickWand> {
    let wand = MagickWand::new();
    wand.read_image(
        path.as_ref()
            .to_str()
            .ok_or(AppError::InvalidUnicode)
            .with_context(|| format!("decoding path: {}", path.as_ref().display()))?,
    )
    .with_context(|| format!("while reading from image at {}", path.as_ref().display()))?;
    Ok(wand)
}

pub fn write_image(wand: &MagickWand, path: impl AsRef<Path>) -> anyhow::Result<()> {
    wand.write_image(
        path.as_ref()
            .to_str()
            .ok_or(AppError::InvalidUnicode)
            .with_context(|| format!("decoding path: {}", path.as_ref().display()))?,
    )
    .with_context(|| format!("while writing to path {}", path.as_ref().display()))?;
    Ok(())
}

pub async fn get_last_modified(path: impl AsRef<Path>) -> DateTime<Utc> {
    tokio::fs::metadata(path.as_ref())
        .await
        .and_then(|m| m.modified())
        .map(|m| m.into())
        .unwrap_or(Utc::now())
}

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_sign_loss)]
#[allow(clippy::cast_possible_truncation)]
pub fn read_and_resize(abs_path: &Path, new_height: usize) -> anyhow::Result<(MagickWand, Vec2)> {
    let abs_path = abs_path
        .to_str()
        .ok_or(AppError::InvalidUnicode)
        .with_context(|| format!("while decoding path: {}", abs_path.display()))?;
    let wand = MagickWand::new();
    wand.read_image(abs_path)
        .with_context(|| format!("while reading from image at {abs_path}"))?;

    let orig_height = wand.get_image_height() as f32;
    let orig_width = wand.get_image_width() as f32;

    let new_width = (orig_width / orig_height * new_height as f32).floor() as usize;

    wand.resize_image(new_width, new_height, FilterType::Lanczos)?;

    Ok((wand, vec2(orig_width, orig_height)))
}

pub fn export_all_rgba(wand: &MagickWand) -> anyhow::Result<Vec<u8>> {
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

pub fn wand_to_image(wand: &MagickWand) -> anyhow::Result<egui::ColorImage> {
    let rgba = export_all_rgba(wand).with_context(|| {
        format!(
            "while reading pixels of {}",
            wand.get_filename().unwrap_or("???".into())
        )
    })?;

    let image = egui::ColorImage::from_rgba_unmultiplied(
        [wand.get_image_width(), wand.get_image_height()],
        &rgba,
    );

    Ok(image)
}
