use crate::data::Item;
use crate::fields;
use chrono::{DateTime, Utc};
use eframe::egui;
use std::ops::Deref;

#[derive(Debug)]
pub struct ThumbnailPosition {
    pub path: String,
    pub last_modified: Option<DateTime<Utc>>,
    pub bounds: egui::Rect,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThumbnailGridParams {
    pub max_row_height: f32,
    pub container_width: f32,
}

impl Default for ThumbnailGridParams {
    fn default() -> Self {
        Self {
            max_row_height: 128.0,
            container_width: 0.0,
        }
    }
}

#[derive(Default, Debug)]
pub struct ThumbnailGridInfo {
    pub is_loading: bool,
    pub params: ThumbnailGridParams,
    pub thumbnails: Vec<ThumbnailPosition>,
}

pub fn compute_thumbnails_grid(
    params: ThumbnailGridParams,
    items: &[impl Deref<Target = Item>],
) -> anyhow::Result<ThumbnailGridInfo> {
    let row_width = params.container_width.floor();
    let row_height = params.max_row_height.floor();
    let mut curr_pos = egui::Pos2::new(0.0, 0.0);
    let mut curr_row = vec![];
    let mut thumbnails = vec![];

    for item in items {
        let Some(size) = item.get_image_size()? else {
            continue;
        };

        let new_size = egui::Vec2::new(size.x / size.y * row_height, row_height);
        let bounds = egui::Rect::from_min_size(curr_pos, new_size);
        curr_row.push(ThumbnailPosition {
            path: item.path().to_string(),
            last_modified: item.get_known_field_value(fields::general::LAST_MODIFIED)?,
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

    Ok(ThumbnailGridInfo {
        is_loading: false,
        params,
        thumbnails,
    })
}
