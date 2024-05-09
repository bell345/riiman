use crate::data::Item;
use crate::fields;
use chrono::{DateTime, Utc};
use eframe::egui;
use eframe::egui::pos2;
use std::ops::Deref;

#[derive(Debug)]
pub struct ThumbnailPosition {
    pub path: String,
    pub last_modified: Option<DateTime<Utc>>,
    pub inner_bounds: egui::Rect,
    pub outer_bounds: egui::Rect,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThumbnailGridParams {
    pub max_row_height: f32,
    pub container_width: f32,
    pub padding: egui::Vec2,
}

impl Default for ThumbnailGridParams {
    fn default() -> Self {
        Self {
            max_row_height: 128.0,
            container_width: Default::default(),
            padding: egui::vec2(4.0, 4.0),
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
    let inner_height = params.max_row_height.floor();
    let mut curr_pos = pos2(0.0, 0.0);
    let mut curr_row = vec![];
    let mut inner_width_sum = 0.0;
    let mut sum_of_ratios = 0.0;
    let mut thumbnails = vec![];

    for item in items {
        let Some(size) = item.get_image_size()? else {
            continue;
        };

        let new_size = egui::Vec2::new(size.x / size.y * inner_height, inner_height);
        let bounds = egui::Rect::from_min_size(curr_pos + params.padding, new_size);
        let outer_bounds = bounds.expand2(params.padding);
        curr_row.push(ThumbnailPosition {
            path: item.path().to_string(),
            last_modified: item.get_known_field_value(fields::general::LAST_MODIFIED)?,
            inner_bounds: bounds,
            outer_bounds,
        });
        curr_pos.x += outer_bounds.size().x;
        sum_of_ratios += bounds.aspect_ratio();
        inner_width_sum += new_size.x;

        if curr_pos.x >= row_width {
            let padding_x_sum = curr_pos.x - inner_width_sum;
            let target_inner_width = row_width - padding_x_sum;
            let new_inner_height = target_inner_width / sum_of_ratios;
            let scale_factor = new_inner_height / inner_height;
            let new_outer_height = new_inner_height + 2.0 * params.padding.y;

            curr_pos.x = 0.0;
            for mut info in curr_row.drain(..) {
                let new_size = (info.inner_bounds.size() * scale_factor).floor();
                info.inner_bounds = egui::Rect::from_min_size(curr_pos + params.padding, new_size);
                info.outer_bounds = info.inner_bounds.expand2(params.padding);
                curr_pos.x += info.outer_bounds.size().x;
                thumbnails.push(info);
            }

            curr_pos.x = 0.0;
            curr_pos.y += new_outer_height;
            inner_width_sum = 0.0;
            sum_of_ratios = 0.0;
        }
    }

    Ok(ThumbnailGridInfo {
        is_loading: false,
        params,
        thumbnails,
    })
}
