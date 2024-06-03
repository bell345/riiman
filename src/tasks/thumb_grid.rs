use crate::data::{FieldStore, Item};
use crate::fields;
use chrono::{DateTime, Utc};
use eframe::egui;
use eframe::egui::{pos2, Pos2};
use std::ops::Deref;

#[derive(Debug)]
pub struct ThumbnailPosition {
    pub path: String,
    pub last_modified: Option<DateTime<Utc>>,
    pub inner_bounds: egui::Rect,
    pub outer_bounds: egui::Rect,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

impl PartialEq for ThumbnailGridParams {
    fn eq(&self, other: &Self) -> bool {
        if self.max_row_height != other.max_row_height {
            return false;
        }

        if (self.container_width - other.container_width).abs() > 5.0 {
            return false;
        }

        if self.padding != other.padding {
            return false;
        }

        true
    }
}

#[derive(Default, Debug)]
pub struct ThumbnailGridInfo {
    pub is_loading: bool,
    pub params: ThumbnailGridParams,
    pub thumbnails: Vec<ThumbnailPosition>,
}

pub fn compute(
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

    let commit_row = |mut row: Vec<ThumbnailPosition>,
                      curr_pos: Pos2,
                      inner_width_sum: f32,
                      sum_of_ratios: f32|
     -> (Pos2, Vec<ThumbnailPosition>) {
        let padding_x_sum = curr_pos.x - inner_width_sum;
        let target_inner_width = row_width - padding_x_sum;
        let new_inner_height = target_inner_width / sum_of_ratios;
        let scale_factor = new_inner_height / inner_height;
        let new_outer_height = new_inner_height + 2.0 * params.padding.y;

        let mut pos = pos2(0.0, curr_pos.y);
        let mut new_row = vec![];
        for mut info in row.drain(..) {
            let new_size = (info.inner_bounds.size() * scale_factor).floor();
            info.inner_bounds = egui::Rect::from_min_size(pos + params.padding, new_size);
            info.outer_bounds = info.inner_bounds.expand2(params.padding);
            pos.x += info.outer_bounds.size().x;
            new_row.push(info);
        }

        (pos2(0.0, curr_pos.y + new_outer_height), new_row)
    };

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
            let (new_curr_pos, new_row) = commit_row(
                std::mem::take(&mut curr_row),
                std::mem::take(&mut curr_pos),
                std::mem::take(&mut inner_width_sum),
                std::mem::take(&mut sum_of_ratios),
            );
            curr_pos = new_curr_pos;
            thumbnails.extend(new_row);
        }
    }

    // last row is centered
    curr_pos.x = (row_width - curr_row.iter().map(|p| p.outer_bounds.width()).sum::<f32>()) / 2.0;
    for mut info in curr_row.drain(..) {
        info.inner_bounds =
            egui::Rect::from_min_size(curr_pos + params.padding, info.inner_bounds.size());
        info.outer_bounds = info.inner_bounds.expand2(params.padding);
        curr_pos.x += info.outer_bounds.size().x;
        thumbnails.push(info);
    }

    Ok(ThumbnailGridInfo {
        is_loading: false,
        params,
        thumbnails,
    })
}
