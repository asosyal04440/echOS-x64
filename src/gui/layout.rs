use crate::gui::protocol::Rect;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeInsets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl EdgeInsets {
    pub const fn all(value: i32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    pub const fn symmetric(horizontal: i32, vertical: i32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlexItem {
    pub basis: u32,
    pub grow: u16,
    pub min_size: u32,
    pub max_size: Option<u32>,
}

impl FlexItem {
    pub const fn fixed(size: u32) -> Self {
        Self {
            basis: size,
            grow: 0,
            min_size: size,
            max_size: Some(size),
        }
    }

    pub const fn fluid(basis: u32, min_size: u32, grow: u16) -> Self {
        Self {
            basis,
            grow,
            min_size,
            max_size: None,
        }
    }
}

pub fn layout_flex(
    bounds: Rect,
    direction: FlexDirection,
    padding: EdgeInsets,
    gap: i32,
    items: &[FlexItem],
) -> Vec<Rect> {
    if items.is_empty() || bounds.is_empty() {
        return Vec::new();
    }

    let inner_x = bounds.x.saturating_add(padding.left);
    let inner_y = bounds.y.saturating_add(padding.top);
    let inner_width = (bounds.width as i32)
        .saturating_sub(padding.left)
        .saturating_sub(padding.right)
        .max(0);
    let inner_height = (bounds.height as i32)
        .saturating_sub(padding.top)
        .saturating_sub(padding.bottom)
        .max(0);

    let available_main = match direction {
        FlexDirection::Row => inner_width,
        FlexDirection::Column => inner_height,
    };
    let available_cross = match direction {
        FlexDirection::Row => inner_height,
        FlexDirection::Column => inner_width,
    };
    let total_gap = gap.saturating_mul(items.len().saturating_sub(1) as i32);
    let usable_main = available_main.saturating_sub(total_gap).max(0);

    let mut sizes = Vec::with_capacity(items.len());
    let mut base_total = 0i32;
    let mut grow_total = 0u32;
    for item in items {
        let mut size = item.basis.max(item.min_size) as i32;
        if let Some(max_size) = item.max_size {
            size = size.min(max_size as i32);
        }
        sizes.push(size.max(0));
        base_total = base_total.saturating_add(size.max(0));
        grow_total = grow_total.saturating_add(item.grow as u32);
    }

    let slack = usable_main.saturating_sub(base_total);
    if slack > 0 && grow_total > 0 {
        let mut distributed = 0i32;
        for (index, item) in items.iter().enumerate() {
            if item.grow == 0 {
                continue;
            }
            let add = ((slack as i64 * item.grow as i64) / grow_total as i64) as i32;
            let candidate = sizes[index].saturating_add(add);
            let capped = item
                .max_size
                .map(|max_size| candidate.min(max_size as i32))
                .unwrap_or(candidate);
            distributed = distributed.saturating_add(capped.saturating_sub(sizes[index]));
            sizes[index] = capped;
        }
        let remainder = slack.saturating_sub(distributed);
        if remainder > 0 {
            for (index, item) in items.iter().enumerate() {
                if item.grow == 0 {
                    continue;
                }
                let next = sizes[index].saturating_add(remainder);
                sizes[index] = item
                    .max_size
                    .map(|max_size| next.min(max_size as i32))
                    .unwrap_or(next);
                break;
            }
        }
    }

    let mut rects = Vec::with_capacity(items.len());
    let mut cursor = 0i32;
    for size in sizes {
        let rect = match direction {
            FlexDirection::Row => Rect::new(
                inner_x.saturating_add(cursor),
                inner_y,
                size.max(0) as u32,
                available_cross.max(0) as u32,
            ),
            FlexDirection::Column => Rect::new(
                inner_x,
                inner_y.saturating_add(cursor),
                available_cross.max(0) as u32,
                size.max(0) as u32,
            ),
        };
        rects.push(rect);
        cursor = cursor.saturating_add(size).saturating_add(gap);
    }

    rects
}

pub fn layout_grid(
    bounds: Rect,
    padding: EdgeInsets,
    columns: usize,
    gap_x: i32,
    gap_y: i32,
    item_count: usize,
    row_height: u32,
) -> Vec<Rect> {
    if columns == 0 || item_count == 0 || bounds.is_empty() {
        return Vec::new();
    }

    let inner_x = bounds.x.saturating_add(padding.left);
    let inner_y = bounds.y.saturating_add(padding.top);
    let inner_width = (bounds.width as i32)
        .saturating_sub(padding.left)
        .saturating_sub(padding.right)
        .max(0);
    let total_gap_x = gap_x.saturating_mul(columns.saturating_sub(1) as i32);
    let column_width = ((inner_width.saturating_sub(total_gap_x)).max(0) / columns as i32).max(0);

    let mut rects = Vec::with_capacity(item_count);
    for index in 0..item_count {
        let column = index % columns;
        let row = index / columns;
        rects.push(Rect::new(
            inner_x.saturating_add(column as i32 * (column_width + gap_x)),
            inner_y.saturating_add(row as i32 * (row_height as i32 + gap_y)),
            column_width as u32,
            row_height,
        ));
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_distributes_slack_to_growing_children() {
        let rects = layout_flex(
            Rect::new(0, 0, 320, 40),
            FlexDirection::Row,
            EdgeInsets::symmetric(10, 4),
            8,
            &[
                FlexItem::fixed(40),
                FlexItem::fluid(80, 60, 1),
                FlexItem::fluid(80, 60, 2),
            ],
        );
        assert_eq!(rects.len(), 3);
        assert!(rects[2].width > rects[1].width);
        assert_eq!(rects[0].x, 10);
        assert_eq!(rects[2].right(), 310);
    }

    #[test]
    fn grid_places_items_in_rows_and_columns() {
        let rects = layout_grid(
            Rect::new(0, 0, 300, 200),
            EdgeInsets::all(10),
            3,
            6,
            8,
            5,
            30,
        );
        assert_eq!(rects.len(), 5);
        assert_eq!(rects[0].x, 10);
        assert_eq!(rects[1].y, 10);
        assert!(rects[3].y > rects[0].y);
    }
}
