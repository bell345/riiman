use std::path::Path;

use chrono::{DateTime, Utc};
use eframe::egui;
use eframe::egui::{pos2, vec2, Align, Direction, Pos2, Vec2};

use crate::data::{FieldStore, Item, ItemId, ThumbnailParams, Vault};
use crate::fields;

#[derive(Debug)]
pub struct ThumbnailPosition {
    pub id: ItemId,
    pub rel_path: String,
    pub abs_path: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
    pub inner_bounds: egui::Rect,
    pub outer_bounds: egui::Rect,
}

impl ThumbnailPosition {
    pub fn new(
        vault: &Vault,
        item: &Item,
        last_modified: Option<DateTime<Utc>>,
        inner_bounds: egui::Rect,
        outer_bounds: egui::Rect,
    ) -> Self {
        let rel_path = item.path().to_string();
        let abs_path = vault.resolve_abs_path(Path::new(item.path())).ok();
        let id = ItemId::from_rel_abs_path(rel_path.as_str(), abs_path.as_ref());

        Self {
            id,
            rel_path,
            abs_path,
            last_modified,
            inner_bounds,
            outer_bounds,
        }
    }

    pub fn params(&self, height: usize) -> Option<ThumbnailParams> {
        Some(ThumbnailParams {
            rel_path: self.rel_path.clone(),
            abs_path: self.abs_path.as_ref()?.clone(),
            last_modified: self.last_modified,
            height,
            transform_params: None,
        })
    }
}

#[derive(Default, Debug)]
pub struct ThumbnailGridInfo {
    pub params: RiverParams,
    pub thumbnails: Vec<ThumbnailPosition>,
}

/// Defines the parameters used for the 'river' algorithm.
/// Note that, by convention, the "main" axis is measured using "width"
/// and the "cross" axis is measured using "height", even if the [`Self::main_axis`]
/// is not [`Direction::LeftToRight`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RiverParams {
    pub container_width: f32,
    pub init_row_height: f32,
    pub item_padding: Vec2,
    pub last_row_align: Align,
    pub main_axis: Direction,
}

impl Default for RiverParams {
    fn default() -> Self {
        Self {
            container_width: Default::default(),
            init_row_height: 128.0,
            item_padding: vec2(4.0, 4.0),
            last_row_align: Align::Center,
            main_axis: Direction::LeftToRight,
        }
    }
}

impl PartialEq for RiverParams {
    fn eq(&self, other: &Self) -> bool {
        if (self.container_width - other.container_width).abs() > 5.0 {
            return false;
        }

        (self.init_row_height, self.item_padding, self.last_row_align)
            == (
                other.init_row_height,
                other.item_padding,
                other.last_row_align,
            )
    }
}

fn get_axis(v: Vec2, axis: Direction) -> f32 {
    if axis.is_horizontal() {
        v.x
    } else {
        v.y
    }
}

fn get_axis_extent(p: Pos2, axis: Direction, max: f32) -> f32 {
    match axis {
        Direction::LeftToRight => p.x,
        Direction::RightToLeft => max - p.x,
        Direction::TopDown => p.y,
        Direction::BottomUp => max - p.y,
    }
}

fn get_cross_axis(main_axis: Direction) -> Direction {
    match main_axis {
        Direction::LeftToRight | Direction::RightToLeft => Direction::TopDown,
        Direction::TopDown | Direction::BottomUp => Direction::LeftToRight,
    }
}

fn reset_axis(p: Pos2, axis: Direction, max: f32) -> Pos2 {
    match axis {
        Direction::LeftToRight => pos2(0.0, p.y),
        Direction::RightToLeft => pos2(max, p.y),
        Direction::TopDown => pos2(p.x, 0.0),
        Direction::BottomUp => pos2(p.x, max),
    }
}

fn inc_axis(p: &mut Pos2, axis: Direction, extent: Vec2) {
    match axis {
        Direction::LeftToRight => {
            p.x += extent.x;
        }
        Direction::RightToLeft => {
            p.x -= extent.x;
        }
        Direction::TopDown => {
            p.y += extent.y;
        }
        Direction::BottomUp => {
            p.y -= extent.y;
        }
    }
}

fn inc_cross_axis(p: Pos2, main_axis: Direction, row_height: f32, max_main: f32) -> Pos2 {
    match main_axis {
        Direction::LeftToRight => pos2(0.0, p.y + row_height),
        Direction::RightToLeft => pos2(max_main, p.y + row_height),
        Direction::TopDown => pos2(p.x + row_height, 0.0),
        Direction::BottomUp => pos2(p.x + row_height, max_main),
    }
}

fn new_rect_on_axis(p: Pos2, padding: Vec2, size: Vec2, axis: Direction) -> egui::Rect {
    match axis {
        Direction::LeftToRight | Direction::TopDown => egui::Rect::from_min_size(p + padding, size),
        Direction::RightToLeft => {
            egui::Rect::from_min_size(pos2(p.x - padding.x - size.x, p.y + padding.y), size)
        }
        Direction::BottomUp => {
            egui::Rect::from_min_size(pos2(p.x + padding.x, p.y - padding.y - size.y), size)
        }
    }
}

fn axis_is_exceeded(p: Pos2, axis: Direction, max: f32) -> bool {
    match axis {
        Direction::LeftToRight => p.x >= max,
        Direction::RightToLeft => p.x <= 0.0,
        Direction::TopDown => p.y >= max,
        Direction::BottomUp => p.y <= 0.0,
    }
}

fn fit_cross_axis(extent: Vec2, main_axis: Direction, row_height: f32) -> Vec2 {
    if main_axis.is_horizontal() {
        vec2(extent.x / extent.y * row_height, row_height)
    } else {
        vec2(row_height, extent.y / extent.x * row_height)
    }
}

fn fix_align(align: Align, axis: Direction) -> Align {
    match (align, axis) {
        (Align::Min, Direction::RightToLeft | Direction::BottomUp) => Align::Max,
        (Align::Max, Direction::RightToLeft | Direction::BottomUp) => Align::Min,
        _ => align,
    }
}

fn fix_aspect_ratio(ratio: f32, axis: Direction) -> f32 {
    if axis.is_horizontal() {
        ratio
    } else {
        1.0 / ratio
    }
}

pub fn river_layout(
    params: &RiverParams,
    vault: &Vault,
    item_ids: &[ItemId],
    get_image_size: impl Fn(&Item) -> Option<Vec2>,
) -> anyhow::Result<ThumbnailGridInfo> {
    let row_width = params.container_width.floor();
    let inner_height = params.init_row_height.floor();
    let main_axis = params.main_axis;
    let cross_axis = get_cross_axis(main_axis);

    let mut curr_pos = reset_axis(pos2(0.0, 0.0), main_axis, row_width);
    let mut curr_row = vec![];
    let mut inner_width_sum = 0.0;
    let mut sum_of_ratios = 0.0;
    let mut thumbnails = vec![];

    let commit_row = |mut row: Vec<ThumbnailPosition>,
                      curr_pos: Pos2,
                      inner_width_sum: f32,
                      sum_of_ratios: f32|
     -> (Pos2, Vec<ThumbnailPosition>) {
        let padding_x_sum = get_axis_extent(curr_pos, main_axis, row_width) - inner_width_sum;
        let target_inner_width = row_width - padding_x_sum;
        let new_inner_height = target_inner_width / sum_of_ratios;
        let scale_factor = new_inner_height / inner_height;
        let new_outer_height = new_inner_height + 2.0 * get_axis(params.item_padding, cross_axis);

        let mut pos = reset_axis(curr_pos, main_axis, row_width);
        let mut new_row = vec![];
        for mut info in row.drain(..) {
            let new_size = (info.inner_bounds.size() * scale_factor).floor();
            info.inner_bounds = new_rect_on_axis(pos, params.item_padding, new_size, main_axis);
            info.outer_bounds = info.inner_bounds.expand2(params.item_padding);
            inc_axis(&mut pos, main_axis, info.outer_bounds.size());
            new_row.push(info);
        }

        (
            inc_cross_axis(curr_pos, main_axis, new_outer_height, row_width),
            new_row,
        )
    };

    for item_id in item_ids {
        let Some(item) = vault.get_item_opt_by_id(*item_id) else {
            continue;
        };

        // TODO: should we ignore non-image items?
        let Some(size) = get_image_size(&item) else {
            continue;
        };

        let last_modified = item.get_known_field_value(fields::general::LAST_MODIFIED)?;

        let new_size = fit_cross_axis(size, main_axis, inner_height);
        let bounds = new_rect_on_axis(curr_pos, params.item_padding, new_size, main_axis);
        let outer_bounds = bounds.expand2(params.item_padding);
        curr_row.push(ThumbnailPosition::new(
            vault,
            &item,
            last_modified,
            bounds,
            outer_bounds,
        ));
        inc_axis(&mut curr_pos, main_axis, outer_bounds.size());
        sum_of_ratios += fix_aspect_ratio(bounds.aspect_ratio(), main_axis);
        inner_width_sum += get_axis(new_size, main_axis);

        if axis_is_exceeded(curr_pos, main_axis, row_width) {
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

    let content_width = curr_row
        .iter()
        .map(|p| get_axis(p.outer_bounds.size(), main_axis))
        .sum::<f32>();

    curr_pos = reset_axis(curr_pos, main_axis, row_width);
    let inc_amount = match fix_align(params.last_row_align, main_axis) {
        Align::Min => 0.0,
        Align::Center => (row_width - content_width) / 2.0,
        Align::Max => row_width - content_width,
    }
    .floor();
    inc_axis(&mut curr_pos, main_axis, Vec2::splat(inc_amount));

    for mut info in curr_row.drain(..) {
        info.inner_bounds = new_rect_on_axis(
            curr_pos,
            params.item_padding,
            info.inner_bounds.size(),
            main_axis,
        );
        info.outer_bounds = info.inner_bounds.expand2(params.item_padding);
        inc_axis(&mut curr_pos, main_axis, info.outer_bounds.size());
        thumbnails.push(info);
    }

    Ok(ThumbnailGridInfo {
        params: params.clone(),
        thumbnails,
    })
}

/*
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MasonryParams {
    pub num_columns: u16,
    pub main_limit: f32,
    pub main_axis: egui::Direction,
    pub padding: egui::Vec2,
}

pub fn masonry_layout(
    params: MasonryParams,
    vault: &Vault,
    item_ids: &[ItemId],
) -> anyhow::Result<ThumbnailGridInfo> {
}*/
