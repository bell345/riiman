use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::{ProgressSenderRef, ProgressState, TaskResult, TaskReturn};
use eframe::egui;

#[derive(Debug)]
pub struct ThumbnailPosition {
    pub path: String,
    pub bounds: egui::Rect,
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct ThumbnailGridParams {
    pub max_row_height: f32,
    pub container_width: f32,
}

#[derive(Default, Debug)]
pub struct ThumbnailGridInfo {
    pub is_loading: bool,
    pub params: ThumbnailGridParams,
    pub thumbnails: Vec<ThumbnailPosition>,
}

pub fn compute_thumbnails_grid(
    params: ThumbnailGridParams,
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> TaskReturn {
    let row_width = params.container_width.floor();
    let row_height = params.max_row_height.floor();
    let mut curr_pos = egui::Pos2::new(0.0, 0.0);
    let mut curr_row = vec![];
    let mut thumbnails = vec![];

    progress.send(ProgressState::Indeterminate);
    let state = state.blocking_read();
    let current_vault = state.get_current_vault().ok_or(AppError::NoCurrentVault)?;

    let n_items = current_vault.len_items();
    progress.send(ProgressState::Determinate(0.0));
    for (i, item) in current_vault.iter_items().enumerate() {
        progress.send(ProgressState::Determinate(i as f32 / n_items as f32));

        let Some(size) = item.get_image_size()? else {
            continue;
        };

        let new_size = egui::Vec2::new(size.x / size.y * row_height, row_height);
        let bounds = egui::Rect::from_min_size(curr_pos, new_size);
        curr_row.push(ThumbnailPosition {
            path: item.path().to_string(),
            bounds,
        });
        curr_pos.x += new_size.x;

        if curr_pos.x >= row_width {
            let new_height = row_width / curr_pos.x * row_height;
            let scale_factor = row_width / curr_pos.x;
            curr_pos.x = 0.0;
            for mut info in curr_row.drain(..) {
                let new_size = (info.bounds.size() * scale_factor).floor();
                info.bounds = egui::Rect::from_min_size(curr_pos, new_size);
                thumbnails.push(info);
                curr_pos.x += new_size.x;
            }
            curr_pos.x = 0.0;
            curr_pos.y += new_height.floor();
        }
    }

    Ok(TaskResult::ThumbnailGrid(ThumbnailGridInfo {
        is_loading: false,
        params,
        thumbnails,
    }))
}
