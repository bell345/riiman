#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]

use crate::data::transform::{
    BulkParams, DestinationKind, DestinationOptions, FitAlgorithm, InfillOptions, InfillTechnique,
    ScaleAlgorithm, ScaleOptions,
};
use crate::data::{
    FieldStore, Item, ItemId, TransformBulkParams, TransformImageParams, TransformPathParams, Vault,
};
use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;
use crate::tasks::image::{export_all_rgba, read_image, wand_to_image};
use crate::tasks::import::{import_single_image, process_many};
use crate::tasks::vault::save_vault_and_links;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};
use anyhow::Context;
use chrono::Utc;
use eframe::egui;
use eframe::egui::{pos2, vec2, Color32, Pos2, Rect, Vec2, ViewportClass};
use magick_rust::{CompositeOperator, FilterType, GravityType, MagickWand, PixelWand};
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::block_in_place;

fn get_integer_scale_factor(original: f32, new: f32) -> f32 {
    if new > original {
        (new / original).floor()
    } else {
        1.0 / (original / new).ceil()
    }
}

fn get_image_size(wand: &MagickWand) -> Vec2 {
    vec2(
        wand.get_image_width() as f32,
        wand.get_image_height() as f32,
    )
}

fn get_scaled_size(size: impl Into<Vec2>, options: &ScaleOptions) -> Vec2 {
    let size = size.into();
    let use_width = options.use_target_width;
    let dst_width = options.target_width as f32;
    let use_height = options.use_target_height;
    let dst_height = options.target_height as f32;

    let src_ratio = size.x / size.y;
    let dst_ratio = dst_width / dst_height;

    let mut new_size = match (use_width, use_height) {
        (true, true) => match (options.fit_algorithm, dst_ratio > src_ratio) {
            (FitAlgorithm::Fill, true) | (FitAlgorithm::Fit, false) => {
                vec2(dst_width, dst_width / src_ratio)
            }
            (FitAlgorithm::Fill, false) | (FitAlgorithm::Fit, true) => {
                vec2(dst_height * src_ratio, dst_height)
            }
            (FitAlgorithm::Stretch, _) => vec2(dst_width, dst_height),
        },
        (false, true) => vec2(dst_height * src_ratio, dst_height),
        (true, false) => vec2(dst_width, dst_width / src_ratio),
        (false, false) => size,
    }
    .floor();

    if options.use_maximum_scaling {
        let max_factor = options.maximum_scaling;
        new_size = new_size.min(size * max_factor.0);
    }

    if options.integer_scaling {
        let int_scale_factor = vec2(
            get_integer_scale_factor(size.x, new_size.x),
            get_integer_scale_factor(size.y, new_size.y),
        );
        new_size = size * int_scale_factor;
    }

    if !options.scale_down {
        new_size = new_size.max(size);
    }

    new_size
}

fn get_infill_size(size: impl Into<Vec2>, options: &InfillOptions) -> Vec2 {
    let size = size.into();
    let src_ratio = size.x / size.y;
    let (dst_x, dst_y) = options.target_aspect_ratio;
    let dst_ratio = dst_x.0 / dst_y.0;

    if dst_ratio > src_ratio {
        vec2(size.y * dst_ratio, size.y)
    } else {
        vec2(size.x, size.x / dst_ratio)
    }
    .floor()
}

fn determine_infill_technique(
    wand: &MagickWand,
    target_size: impl Into<Vec2>,
    colour_tolerance: f64,
) -> anyhow::Result<InfillTechnique> {
    let colour_tolerance = colour_tolerance.clamp(0.0, 1.0);

    let source_size = get_image_size(wand);
    let source_ratio = source_size.x / source_size.y;
    let target_size = target_size.into();
    let target_ratio = target_size.x / target_size.y;

    let trim_wand = MagickWand::new_from_image(&wand.get_image()?)?;
    trim_wand.trim_image(colour_tolerance)?;
    let trim_size = get_image_size(&trim_wand);

    match (
        target_ratio < source_ratio,
        trim_size.x < source_size.x,
        trim_size.y < source_size.y,
    ) {
        (true, _, trimmed) | (false, trimmed, _) => {
            if trimmed {
                Ok(InfillTechnique::Solid)
            } else {
                Ok(InfillTechnique::Blur)
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn pixel_wand_set_color32(pixel_wand: &mut PixelWand, colour: Color32) {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    pixel_wand.set_red(r as f64 / u8::MAX as f64);
    pixel_wand.set_green(g as f64 / u8::MAX as f64);
    pixel_wand.set_blue(b as f64 / u8::MAX as f64);
    pixel_wand.set_alpha(a as f64 / u8::MAX as f64);
}

fn determine_infill_colour(wand: &MagickWand, params: &InfillOptions) -> PixelWand {
    const INFILL_SOURCE: Pos2 = pos2(3.0, 3.0);

    if params.use_auto_solid {
        wand.get_image_pixel_color(INFILL_SOURCE.x as isize, INFILL_SOURCE.y as isize)
            .unwrap_or_else(PixelWand::new)
    } else {
        let mut pixel_wand = PixelWand::new();
        pixel_wand_set_color32(&mut pixel_wand, params.manual_solid_colour);
        pixel_wand
    }
}

fn do_solid_infill(wand: &mut MagickWand, params: &InfillOptions) -> anyhow::Result<()> {
    let size = get_image_size(wand);
    let in_ratio = size.x / size.y;
    let out_size = get_infill_size(size, params);
    let out_ratio = out_size.x / out_size.y;

    let colour = determine_infill_colour(wand, params);
    let (border_width, border_height) = if in_ratio > out_ratio {
        // horizontal becomes vertical
        (0.0, (out_size.y - size.y) / 2.0)
    } else {
        // vertical becomes horizontal
        ((out_size.x - size.x) / 2.0, 0.0)
    };

    wand.border_image(
        &colour,
        border_width as usize,
        border_height as usize,
        CompositeOperator::Over,
    )?;

    Ok(())
}

fn do_blur_infill(
    wand: &mut MagickWand,
    params: &InfillOptions,
    blur_scale: Option<f32>,
) -> anyhow::Result<()> {
    let size = get_image_size(wand);
    let in_ratio = size.x / size.y;
    let out_size = get_infill_size(size, params);
    let out_ratio = out_size.x / out_size.y;

    let crop_rect = if in_ratio > out_ratio {
        // vertical center crop
        Rect::from_min_size(
            pos2((out_size.x - size.x) / 2.0, 0.0).floor(),
            vec2(size.y * out_ratio, size.y).floor(),
        )
    } else {
        // horizontal center crop
        Rect::from_min_size(
            pos2(0.0, (out_size.y - size.y) / 2.0).floor(),
            vec2(size.x, size.x / out_ratio).floor(),
        )
    };

    let orig_wand = MagickWand::new_from_image(&wand.get_image()?)?;
    wand.crop_image(
        crop_rect.width() as usize,
        crop_rect.height() as usize,
        crop_rect.min.x as isize,
        crop_rect.min.y as isize,
    )?;

    if params.use_gaussian {
        let mut radius = params.gaussian_radius as f64;
        if let Some(blur_scale) = blur_scale {
            radius *= blur_scale as f64;
        }
        wand.blur_image(0.0, radius)?;
    }

    let brightness = if params.use_brightness {
        params.brightness_change.0 * 100.0
    } else {
        0.0
    };
    let contrast = if params.use_contrast {
        params.contrast_change.0 * 100.0
    } else {
        0.0
    };
    wand.brightness_contrast_image(brightness as f64, contrast as f64)?;

    wand.resize_image(
        out_size.x as usize,
        out_size.y as usize,
        FilterType::Lanczos,
    )?;

    wand.compose_images_gravity(&orig_wand, CompositeOperator::Over, GravityType::Center)?;

    Ok(())
}

pub fn get_transformed_size(size: impl Into<Vec2>, params: &TransformImageParams) -> Vec2 {
    let mut size = size.into();

    if params.scale.enabled {
        size = get_scaled_size(size, &params.scale);
    }

    if params.infill.enabled {
        size = get_infill_size(size, &params.infill);
    }

    size
}

fn load_image_preview_task(
    abs_path: impl AsRef<Path>,
    params: Option<&TransformImageParams>,
) -> AsyncTaskReturn {
    let mut wand = read_image(abs_path)?;
    if let Some(params) = params {
        transform_wand(&mut wand, params, None)?;
    }
    let image = wand_to_image(&wand)?;
    Ok(AsyncTaskResult::PreviewReady {
        id: egui::Id::new("preview_image"),
        image,
        viewport_class: ViewportClass::Deferred.into(),
    })
}

pub fn load_image_preview(abs_path: impl AsRef<Path>) -> AsyncTaskReturn {
    block_in_place(|| load_image_preview_task(abs_path, None))
}

pub fn load_transformed_image_preview(
    abs_path: impl AsRef<Path>,
    params: &TransformImageParams,
) -> AsyncTaskReturn {
    block_in_place(|| load_image_preview_task(abs_path, Some(params)))
}

fn scale_with_xbrz(
    wand: &mut MagickWand,
    target_width: usize,
    target_height: usize,
) -> anyhow::Result<()> {
    let mut width = wand.get_image_width();
    let mut height = wand.get_image_height();
    let width_factor = target_width / width;
    let height_factor = target_height / height;
    let factor = width_factor.min(height_factor).min(6);

    if factor > 1 {
        let rgba = export_all_rgba(wand)?;
        let new_rgba = xbrz::scale_rgba(&rgba, width, height, factor);
        width *= factor;
        height *= factor;
        wand.extend_image(width, height, 0, 0)?;
        wand.import_image_pixels(0, 0, width, height, &new_rgba, "RGBA")?;
    }

    if width != target_width || height != target_height {
        wand.resize_image(target_width, target_height, FilterType::Lanczos)?;
    }

    // TODO: allow intermediate scaling algorithm to be chosen

    Ok(())
}

pub fn transform_wand(
    wand: &mut MagickWand,
    params: &TransformImageParams,
    full_size: Option<Vec2>,
) -> anyhow::Result<()> {
    let orig_size = get_image_size(wand);

    if params.scale.enabled {
        let mut scaled_size = get_scaled_size(orig_size, &params.scale);
        if full_size.is_some() {
            scaled_size = get_scaled_size(
                scaled_size,
                &ScaleOptions {
                    enabled: true,
                    use_target_width: true,
                    target_width: orig_size.x as u32,
                    use_target_height: true,
                    target_height: orig_size.y as u32,
                    scale_down: true,
                    fit_algorithm: FitAlgorithm::Fit,
                    ..Default::default()
                },
            );
        }

        let (width, height) = (scaled_size.x as usize, scaled_size.y as usize);
        match params.scale.scale_algorithm {
            ScaleAlgorithm::NearestNeighbour => wand.sample_image(width, height)?,
            ScaleAlgorithm::Bilinear => wand.resize_image(width, height, FilterType::Triangle)?,
            ScaleAlgorithm::Bicubic => wand.resize_image(width, height, FilterType::Catrom)?,
            ScaleAlgorithm::Xbrz => scale_with_xbrz(wand, width, height)?,
            ScaleAlgorithm::Esrgan => todo!(),
        }
    }

    if params.infill.enabled {
        let out_size = get_infill_size(get_image_size(wand), &params.infill);
        let technique = match params.infill.technique {
            // TODO: allow choosing infill colour tolerance?
            InfillTechnique::Automatic => determine_infill_technique(wand, out_size, 0.02)?,
            tech => tech,
        };

        // assume x scale == y scale
        let blur_scale = full_size.map(|full_size| orig_size.x / full_size.x);

        match technique {
            InfillTechnique::Blur => do_blur_infill(wand, &params.infill, blur_scale)?,
            InfillTechnique::Solid => do_solid_infill(wand, &params.infill)?,
            InfillTechnique::Automatic => unreachable!(),
        };
    }

    Ok(())
}

pub fn list_destination_paths(
    dest: &DestinationOptions,
    app_state: AppStateRef,
) -> anyhow::Result<Vec<PathBuf>> {
    Ok(match dest.kind {
        k @ (DestinationKind::SameVault | DestinationKind::OtherVault) => {
            let vault = if k == DestinationKind::SameVault {
                app_state.current_vault()?
            } else {
                app_state.get_vault(&dest.other_vault_name)?
            };
            vault
                .iter_items()
                .map(|i| Path::new(i.path()).to_path_buf())
                .filter(|p| !dest.use_subdirectory || p.starts_with(&dest.vault_subdirectory))
                .collect()
        }
        DestinationKind::Directory => {
            let mut entries = vec![];
            let mut ret = vec![];
            entries.extend(
                fs::read_dir(Path::new(&dest.directory_path))?.map(|r| (PathBuf::new(), r)),
            );
            while let Some((subdir, entry)) = entries.pop() {
                let Ok(entry) = entry else {
                    continue;
                };
                let Ok(ft) = entry.file_type() else {
                    continue;
                };

                if ft.is_file() {
                    ret.push(subdir.join(entry.file_name()));
                } else if ft.is_dir() {
                    let new_subdir = subdir.join(entry.file_name());
                    let Ok(sub_entries) = fs::read_dir(entry.path()) else {
                        continue;
                    };
                    entries.extend(sub_entries.map(|r| (new_subdir.clone(), r)));
                }
            }
            ret
        }
        DestinationKind::Archive => {
            // TODO: only zip archives supported
            let fp = fs::File::open(Path::new(&dest.archive_path))?;
            let buf_reader = BufReader::new(fp);
            let ar = zip::ZipArchive::new(buf_reader)?;
            ar.file_names().map(PathBuf::from).collect()
        }
    })
}

pub fn transform_path(item: &Item, params: &TransformPathParams) -> Option<PathBuf> {
    // TODO: Replace hardcoded implementation
    let author_id = item
        .get_known_field_value(fields::tweet::AUTHOR_ID)
        .ok()??;
    let post_id = item.get_known_field_value(fields::tweet::ID).ok()??;
    let img_num = item
        .get_known_field_value(fields::tweet::IMAGE_NUMBER)
        .ok()??;
    let extension = Path::new(item.path()).extension()?.to_str()?;
    Some(format!("twitter_{author_id}_{post_id}_{img_num}.{extension}").into())
}

#[derive(Debug)]
pub enum PathTransformResult {
    NoTransform(PathBuf),
    RemovedWithoutTransform(PathBuf),
    MoveSuccess { removed: PathBuf, created: PathBuf },
    CopySuccess { original: PathBuf, copy: PathBuf },
}

struct Discriminator<'a> {
    parent: &'a Path,
    base_name: &'a OsStr,
    extension: Option<&'a OsStr>,
    discriminator: Option<usize>,
}

impl<'a> Discriminator<'a> {
    fn from_path(path: &'a Path) -> Option<Self> {
        let parent = path.parent()?;
        let base_name = path.file_stem()?;
        let extension = path.extension();
        Some(Self {
            parent,
            base_name,
            extension,
            discriminator: None,
        })
    }

    fn get_path(&self) -> PathBuf {
        let mut base_name = self.base_name.to_owned();
        if let Some(disc) = self.discriminator.as_ref() {
            base_name.push(format!(" ({disc})"));
        }
        if let Some(ext) = self.extension.as_ref() {
            base_name.push(".");
            base_name.push(ext);
        }
        self.parent.join(base_name)
    }

    fn increment(&mut self) {
        self.discriminator = Some(self.discriminator.map_or(2, |x| x + 1));
    }

    fn into_unique_path(mut self, check_fn: impl Fn(&Path) -> Option<bool>) -> Option<PathBuf> {
        loop {
            let path = self.get_path();
            if check_fn(&path)? {
                return Some(path);
            }
            self.increment();
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn apply_path_transformation(
    state: AppStateRef,
    vault: Arc<Vault>,
    item_id: ItemId,
    bulk: Arc<BulkParams>,
    params: Arc<TransformPathParams>,
) -> anyhow::Result<PathTransformResult> {
    use self::PathTransformResult::*;
    use crate::data::transform::DestinationExistingBehaviour::*;

    let old_item = vault.get_item_by_id(item_id)?;
    let old_abs_path = vault.resolve_abs_path(Path::new(old_item.path()))?;
    let Some(mut new_path) = transform_path(&old_item, &params) else {
        return Ok(NoTransform(old_abs_path));
    };

    let exist_behaviour = bulk.destination.item_existing_behaviour;
    let dry_run = params.dry_run;

    macro_rules! remove {
        () => {
            if !dry_run {
                vault.remove_item(&Path::new(old_item.path()))?;
                state.unlink_item(&old_item)?;
                tokio::fs::remove_file(&old_abs_path).await?;
            }
        };
    }

    match bulk.destination.kind {
        k @ (DestinationKind::SameVault | DestinationKind::OtherVault) => {
            if k == DestinationKind::SameVault && new_path.as_os_str() == old_item.path() {
                return Ok(NoTransform(old_abs_path));
            }

            let other_vault = match k {
                DestinationKind::SameVault => Arc::clone(&vault),
                DestinationKind::OtherVault => {
                    state.get_vault(&bulk.destination.other_vault_name)?
                }
                _ => unreachable!(),
            };

            if exist_behaviour == AppendDiscriminator {
                let Some(disc_path) = Discriminator::from_path(&new_path)
                    .and_then(|d| d.into_unique_path(|p| Some(other_vault.get_item(p).is_err())))
                else {
                    return Ok(NoTransform(old_abs_path));
                };
                new_path = disc_path;
            }

            let other_item_opt = other_vault.get_item(&new_path).ok();
            let new_abs_path = other_vault.resolve_abs_path(&new_path)?;
            let dest_file_exists = tokio::fs::try_exists(&new_abs_path).await?;

            macro_rules! move_or_copy_into {
                ($item:ident) => {
                    if !dry_run {
                        tokio::fs::copy(&old_abs_path, &new_abs_path).await?;
                        $item.update(old_item.as_ref());
                    }
                    if bulk.source.delete_source {
                        if !dry_run {
                            vault.remove_item(&Path::new(old_item.path()))?;
                            state.update_item_link(&other_vault, &$item)?;
                            tokio::fs::remove_file(&old_abs_path).await?;
                        }
                        return Ok(MoveSuccess {
                            removed: old_abs_path,
                            created: new_abs_path,
                        });
                    }

                    return Ok(CopySuccess {
                        original: old_abs_path,
                        copy: new_abs_path,
                    });
                };
            }

            // if item+file exists:
            //      fall through, use exist_behaviour
            // if item exists, but file does not:
            //      unexpected condition, return error
            // if file exists but item does not:
            //      import item for other_vault, and then fall through and use exist_behaviour
            // if neither exists:
            //      create item for other_vault based on old_item

            let other_item = match (other_item_opt, dest_file_exists) {
                (Some(item), true) => item,
                (Some(_), false) => {
                    return Err(AppError::MissingFile {
                        abs_path: new_abs_path,
                    }
                    .into())
                }
                (None, true) => {
                    if dry_run {
                        return Err(AppError::MissingItem {
                            path: new_path.to_string_lossy().into(),
                        }
                        .into());
                    }

                    let last_modified = tokio::fs::metadata(&new_abs_path)
                        .await
                        .and_then(|m| m.modified())
                        .map(|m| m.into())
                        .unwrap_or(Utc::now());
                    import_single_image(
                        Arc::clone(&other_vault),
                        new_path.clone().into_boxed_path(),
                        last_modified,
                    )
                    .await?;
                    other_vault.get_item(&new_path)?
                }
                (None, false) => {
                    let new_item = if dry_run {
                        Arc::clone(&old_item)
                    } else {
                        other_vault.get_item_or_init(Path::new(&new_path))?
                    };
                    move_or_copy_into!(new_item);
                }
            };

            match exist_behaviour {
                Remove if bulk.source.delete_source => {
                    remove!();
                    Ok(RemovedWithoutTransform(old_abs_path))
                }
                Skip | Remove => Ok(NoTransform(old_abs_path)),
                Overwrite | AppendDiscriminator => {
                    move_or_copy_into!(other_item);
                }
            }
        }
        DestinationKind::Directory => {
            let mut new_abs_path =
                Path::new(&bulk.destination.directory_path).join(Path::new(&new_path));

            if exist_behaviour == AppendDiscriminator {
                let Some(disc_path) = Discriminator::from_path(&new_abs_path).and_then(|d| {
                    d.into_unique_path(|p| match fs::metadata(p) {
                        Ok(_) => Some(false),
                        Err(e) if e.kind() == ErrorKind::NotFound => Some(true),
                        Err(_) => None,
                    })
                }) else {
                    return Ok(NoTransform(old_abs_path));
                };
                new_abs_path = disc_path;
            }

            let dest_file_exists = tokio::fs::try_exists(&new_abs_path).await?;

            match (dest_file_exists, exist_behaviour) {
                (true, Remove) if bulk.source.delete_source => {
                    remove!();
                    Ok(RemovedWithoutTransform(old_abs_path))
                }
                (true, Remove | Skip) => Ok(NoTransform(old_abs_path)),
                (false, _) | (true, Overwrite | AppendDiscriminator) => {
                    if !dry_run {
                        tokio::fs::copy(&old_abs_path, &new_abs_path).await?;
                    }
                    if bulk.source.delete_source {
                        remove!();
                        return Ok(MoveSuccess {
                            removed: old_abs_path,
                            created: new_abs_path,
                        });
                    }

                    Ok(CopySuccess {
                        original: old_abs_path,
                        copy: new_abs_path,
                    })
                }
            }
        }
        DestinationKind::Archive => todo!(),
    }
}

#[derive(Debug)]
pub struct PathContext(pub PathBuf);

impl Display for PathContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

async fn apply_path_transformation_wrap(
    state: AppStateRef,
    vault: Arc<Vault>,
    item_id: ItemId,
    bulk: Arc<BulkParams>,
    params: Arc<TransformPathParams>,
) -> anyhow::Result<PathTransformResult> {
    let path = vault.resolve_abs_path(Path::new(vault.get_item_by_id(item_id)?.path()))?;
    apply_path_transformation(state, vault, item_id, bulk, params)
        .await
        .with_context(|| PathContext(path))
}

const CONCURRENT_TASKS_LIMIT: usize = 16;

pub async fn apply_path_transformations(
    state: AppStateRef,
    vault: Arc<Vault>,
    item_ids: Vec<ItemId>,
    bulk: TransformBulkParams,
    params: TransformPathParams,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let bulk = Arc::new(bulk);
    let params = Arc::new(params);
    let results = process_many(
        item_ids,
        progress.sub_task("Import", 0.90),
        |id| {
            apply_path_transformation_wrap(
                state.clone(),
                Arc::clone(&vault),
                id,
                Arc::clone(&bulk),
                Arc::clone(&params),
            )
        },
        |_result, progress, p| {
            progress.send(ProgressState::Determinate(p));
        },
        CONCURRENT_TASKS_LIMIT,
    )
    .await?;

    save_vault_and_links(state, vault, progress.sub_task("Save", 0.05)).await?;

    Ok(AsyncTaskResult::PathTransformationComplete(results))
}

#[cfg(test)]
mod test {
    use super::*;

    fn scale_params(f: impl FnOnce(&mut ScaleOptions)) -> TransformImageParams {
        let mut scale_options = ScaleOptions {
            enabled: true,
            use_target_width: true,
            use_target_height: true,
            target_width: 1920,
            target_height: 1080,
            ..Default::default()
        };
        f(&mut scale_options);
        TransformImageParams {
            scale: scale_options,
            ..Default::default()
        }
    }

    fn infill_params(f: impl FnOnce(&mut InfillOptions)) -> TransformImageParams {
        let mut infill_options = InfillOptions {
            enabled: true,
            ..Default::default()
        };
        f(&mut infill_options);
        TransformImageParams {
            infill: infill_options,
            ..Default::default()
        }
    }

    fn test(p: &TransformImageParams, input: impl Into<Vec2>, output: impl Into<Vec2>) {
        assert_eq!(get_transformed_size(input.into(), p), output.into());
    }

    #[test]
    fn test_fill_algorithm() {
        let p = scale_params(|s| s.fit_algorithm = FitAlgorithm::Fill);

        test(&p, (500.0, 500.0), (1920.0, 1920.0));
        test(&p, (200.0, 1600.0), (1920.0, 15360.0));
        test(&p, (2000.0, 1000.0), (2160.0, 1080.0));
    }

    #[test]
    fn test_fit_algorithm() {
        let p = scale_params(|s| s.fit_algorithm = FitAlgorithm::Fit);

        test(&p, (500.0, 500.0), (1080.0, 1080.0));
        test(&p, (20.0, 160.0), (135.0, 1080.0));
        test(&p, (200.0, 100.0), (1920.0, 960.0));
    }

    #[test]
    fn test_stretch_algorithm() {
        let p = scale_params(|s| s.fit_algorithm = FitAlgorithm::Stretch);

        test(&p, (500.0, 500.0), (1920.0, 1080.0));
        test(&p, (20.0, 160.0), (1920.0, 1080.0));
        test(&p, (200.0, 100.0), (1920.0, 1080.0));
    }

    #[test]
    fn test_scale_down() {
        let mut p = scale_params(|s| {
            s.fit_algorithm = FitAlgorithm::Fit;
            s.scale_down = false;
        });

        test(&p, (5000.0, 5000.0), (5000.0, 5000.0));
        test(&p, (2000.0, 16000.0), (2000.0, 16000.0));
        test(&p, (200.0, 1600.0), (200.0, 1600.0));
        test(&p, (200.0, 100.0), (1920.0, 960.0));

        p.scale.scale_down = true;
        test(&p, (5000.0, 5000.0), (1080.0, 1080.0));
        test(&p, (2000.0, 16000.0), (135.0, 1080.0));
        test(&p, (200.0, 1600.0), (135.0, 1080.0));
        test(&p, (200.0, 100.0), (1920.0, 960.0));

        p.scale.fit_algorithm = FitAlgorithm::Fill;
        p.scale.scale_down = false;
        test(&p, (5000.0, 5000.0), (5000.0, 5000.0));
        test(&p, (2000.0, 16000.0), (2000.0, 16000.0));
        test(&p, (200.0, 1600.0), (1920.0, 15360.0));
        test(&p, (200.0, 100.0), (2160.0, 1080.0));

        p.scale.scale_down = true;
        test(&p, (5000.0, 5000.0), (1920.0, 1920.0));
        test(&p, (2000.0, 16000.0), (1920.0, 15360.0));
        test(&p, (200.0, 1600.0), (1920.0, 15360.0));
        test(&p, (200.0, 100.0), (2160.0, 1080.0));

        p.scale.fit_algorithm = FitAlgorithm::Stretch;
        p.scale.scale_down = false;
        test(&p, (5000.0, 5000.0), (5000.0, 5000.0));
        test(&p, (2000.0, 16000.0), (2000.0, 16000.0));
        test(&p, (200.0, 1600.0), (1920.0, 1600.0));
        test(&p, (200.0, 100.0), (1920.0, 1080.0));

        p.scale.scale_down = true;
        test(&p, (5000.0, 5000.0), (1920.0, 1080.0));
        test(&p, (2000.0, 16000.0), (1920.0, 1080.0));
        test(&p, (200.0, 1600.0), (1920.0, 1080.0));
        test(&p, (200.0, 100.0), (1920.0, 1080.0));
    }

    #[test]
    fn test_maximum_scaling() {
        let p = scale_params(|s| {
            s.fit_algorithm = FitAlgorithm::Fill;
            s.use_maximum_scaling = true;
            s.maximum_scaling = 2.0.into();
            s.scale_down = true;
        });

        test(&p, (5000.0, 5000.0), (1920.0, 1920.0));
        test(&p, (500.0, 500.0), (1000.0, 1000.0));
        test(&p, (200.0, 1600.0), (400.0, 3200.0));
        test(&p, (200.0, 100.0), (400.0, 200.0));
    }

    #[test]
    fn test_integer_scaling() {
        let p = scale_params(|s| {
            s.fit_algorithm = FitAlgorithm::Fit;
            s.integer_scaling = true;
            s.scale_down = true;
        });

        test(&p, (5000.0, 5000.0), (1000.0, 1000.0));
        test(&p, (500.0, 500.0), (1000.0, 1000.0));
        test(&p, (200.0, 1600.0), (100.0, 800.0));
        test(&p, (200.0, 100.0), (1800.0, 900.0));
    }

    #[test]
    fn test_only_target_width() {
        let mut p = scale_params(|s| {
            s.use_target_height = false;
            s.scale_down = true;
        });

        test(&p, (500.0, 500.0), (1920.0, 1920.0));
        test(&p, (200.0, 1600.0), (1920.0, 15360.0));
        test(&p, (2000.0, 1000.0), (1920.0, 960.0));

        p.scale.scale_down = false;
        test(&p, (500.0, 500.0), (1920.0, 1920.0));
        test(&p, (200.0, 1600.0), (1920.0, 15360.0));
        test(&p, (2000.0, 1000.0), (2000.0, 1000.0));
    }

    #[test]
    fn test_only_target_height() {
        let mut p = scale_params(|s| {
            s.use_target_width = false;
            s.scale_down = true;
        });

        test(&p, (500.0, 500.0), (1080.0, 1080.0));
        test(&p, (200.0, 1600.0), (135.0, 1080.0));
        test(&p, (2000.0, 1000.0), (2160.0, 1080.0));

        p.scale.scale_down = false;
        test(&p, (500.0, 500.0), (1080.0, 1080.0));
        test(&p, (200.0, 1600.0), (200.0, 1600.0));
        test(&p, (2000.0, 1000.0), (2160.0, 1080.0));
    }

    #[test]
    fn test_infill() {
        let p = infill_params(|i| {
            i.target_aspect_ratio = (2.0.into(), 1.0.into());
        });

        test(&p, (500.0, 500.0), (1000.0, 500.0));
        test(&p, (200.0, 1600.0), (3200.0, 1600.0));
        test(&p, (1600.0, 200.0), (1600.0, 800.0));
        test(&p, (2000.0, 1000.0), (2000.0, 1000.0));
    }
}
