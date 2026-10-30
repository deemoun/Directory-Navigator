//! DN-style layouts, planned completely before any native shell changes.
use crate::presentation::layout::Rect;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    Tile,
    Cascade,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    NoWindows,
    NoRoom,
    Blocked,
}

#[cfg(test)]
fn plan(area: Rect, minimums: &[(u16, u16)], mode: Arrangement) -> Result<Vec<Rect>, Error> {
    windows(
        area,
        &minimums
            .iter()
            .map(|&(w, h)| super::geometry::Capabilities::document(w, h))
            .collect::<Vec<_>>(),
        mode,
    )
}

/// Fixed-size documents participate in placement without stretching their content.
pub(crate) fn windows(
    area: Rect,
    capabilities: &[super::geometry::Capabilities],
    mode: Arrangement,
) -> Result<Vec<Rect>, Error> {
    if capabilities.is_empty() {
        return Err(Error::NoWindows);
    }
    if area.width == 0 || area.height == 0 {
        return Err(Error::NoRoom);
    }
    let minimums = capabilities.iter().map(|c| c.minimum).collect::<Vec<_>>();
    if mode == Arrangement::Tile {
        let mut rects = match tile(area, &minimums) {
            Ok(rects) => rects,
            Err(Error::NoRoom) => return tile_with_fixed_strip(area, capabilities),
            Err(error) => return Err(error),
        };
        for (r, cap) in rects.iter_mut().zip(capabilities) {
            if !cap.resize {
                r.width = cap.minimum.0;
                r.height = cap.minimum.1;
            }
        }
        return Ok(rects);
    }
    capabilities
        .iter()
        .enumerate()
        .map(|(index, cap)| {
            let offset = u16::try_from(index).map_err(|_| Error::NoRoom)?;
            if cap.resize {
                let width = area.width.checked_sub(offset).ok_or(Error::NoRoom)?;
                let height = area.height.checked_sub(offset).ok_or(Error::NoRoom)?;
                if width < cap.minimum.0 || height < cap.minimum.1 {
                    return Err(Error::NoRoom);
                }
                Ok(Rect::new(area.x + offset, area.y + offset, width, height))
            } else {
                let dx = area.width.checked_sub(cap.minimum.0).ok_or(Error::NoRoom)?;
                let dy = area
                    .height
                    .checked_sub(cap.minimum.1)
                    .ok_or(Error::NoRoom)?;
                // A full-height game still fits at 80x25: offset only as far as
                // space permits, rather than rejecting or clipping its bottom row.
                Ok(Rect::new(
                    area.x + offset.min(dx),
                    area.y + offset.min(dy),
                    cap.minimum.0,
                    cap.minimum.1,
                ))
            }
        })
        .collect()
}
// Equal-count columns can reject an otherwise valid mixed layout: a tall
// fixed game needs its own column while shorter documents stack alongside it.
// Try reserving a strip before reporting failure; all planning stays atomic.
fn tile_with_fixed_strip(
    area: Rect,
    capabilities: &[super::geometry::Capabilities],
) -> Result<Vec<Rect>, Error> {
    let (index, fixed) = capabilities
        .iter()
        .enumerate()
        .find(|(_, c)| !c.resize)
        .ok_or(Error::NoRoom)?;
    let (w, h) = fixed.minimum;
    if w > area.width || h > area.height {
        return Err(Error::NoRoom);
    }
    let others = capabilities
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, c)| *c)
        .collect::<Vec<_>>();
    for remaining in [
        Rect::new(area.x + w, area.y, area.width - w, area.height),
        Rect::new(area.x, area.y + h, area.width, area.height - h),
    ] {
        if let Ok(rects) = windows(remaining, &others, Arrangement::Tile) {
            let mut rects = rects.into_iter();
            return Ok((0..capabilities.len())
                .map(|i| {
                    if i == index {
                        Rect::new(area.x, area.y, w, h)
                    } else {
                        rects.next().expect("one rectangle per planned document")
                    }
                })
                .collect());
        }
    }
    Err(Error::NoRoom)
}
/// DN's divider positions remain equally spaced when they fit. Different
/// document minimums may borrow space from their neighbours, without gaps.
fn partition(total: u16, minimums: &[u16]) -> Option<Vec<u16>> {
    let count = minimums.len() as u32;
    let equal = (0..count)
        .map(|i| ((u32::from(total) * (i + 1) / count) - (u32::from(total) * i / count)) as u16)
        .collect::<Vec<_>>();
    if equal
        .iter()
        .zip(minimums)
        .all(|(size, min)| *size >= (*min).max(1))
    {
        return Some(equal);
    }
    let needed = minimums.iter().map(|m| u32::from((*m).max(1))).sum::<u32>();
    let remaining = u32::from(total).checked_sub(needed)?;
    Some(
        minimums
            .iter()
            .enumerate()
            .map(|(i, min)| {
                let i = i as u32;
                (*min).max(1) + (remaining * (i + 1) / count - remaining * i / count) as u16
            })
            .collect(),
    )
}
fn tile(area: Rect, minimums: &[(u16, u16)]) -> Result<Vec<Rect>, Error> {
    let count = minimums.len();
    let needed = minimums
        .iter()
        .map(|(w, h)| u64::from((*w).max(1)) * u64::from((*h).max(1)))
        .sum::<u64>();
    if needed > u64::from(area.width) * u64::from(area.height) {
        return Err(Error::NoRoom);
    }
    let max_columns = count.min(usize::from(
        area.width / minimums.iter().map(|m| m.0.max(1)).min().unwrap(),
    ));
    let mut columns = (1..=max_columns).collect::<Vec<_>>();
    columns.sort_by_key(|&c| (c.abs_diff(count.div_ceil(c)), std::cmp::Reverse(c)));
    for columns in columns {
        // As in TDesktop.Tile: the last remainder columns get one extra row.
        let rows = (0..columns)
            .map(|c| count / columns + usize::from(c >= columns - count % columns))
            .collect::<Vec<_>>();
        let mut start = 0;
        let widths = rows
            .iter()
            .map(|&n| {
                let w = minimums[start..start + n]
                    .iter()
                    .map(|m| m.0)
                    .max()
                    .unwrap();
                start += n;
                w
            })
            .collect::<Vec<_>>();
        let Some(widths) = partition(area.width, &widths) else {
            continue;
        };
        let mut output = Vec::with_capacity(count);
        let mut start = 0;
        let mut x = area.x;
        for (width, rows) in widths.into_iter().zip(rows) {
            let mins = minimums[start..start + rows]
                .iter()
                .map(|m| m.1)
                .collect::<Vec<_>>();
            let Some(heights) = partition(area.height, &mins) else {
                break;
            };
            let mut y = area.y;
            for height in heights {
                output.push(Rect::new(x, y, width, height));
                y += height;
            }
            start += rows;
            x += width;
        }
        if output.len() == count {
            return Ok(output);
        }
    }
    Err(Error::NoRoom)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_aware_tiles_fill_translated_desktops_without_overlaps() {
        for (w, h) in [(80, 23), (100, 30), (132, 38), (201, 73)] {
            for n in 1..=12 {
                let minimums = (0..n)
                    .map(|i| if i % 3 == 0 { (72, 12) } else { (48, 10) })
                    .collect::<Vec<_>>();
                let area = Rect::new(3, 4, w, h);
                if let Ok(rects) = plan(area, &minimums, Arrangement::Tile) {
                    assert_eq!(rects.len(), n);
                    assert_eq!(
                        rects
                            .iter()
                            .map(|r| u32::from(r.width) * u32::from(r.height))
                            .sum::<u32>(),
                        u32::from(w) * u32::from(h)
                    );
                    for (i, r) in rects.iter().enumerate() {
                        assert_eq!(r.intersection(area), *r);
                        assert!(r.width >= minimums[i].0 && r.height >= minimums[i].1);
                        for other in &rects[i + 1..] {
                            assert!(r.intersection(*other).is_empty());
                        }
                    }
                    assert_eq!(plan(area, &minimums, Arrangement::Tile), Ok(rects));
                }
            }
        }
    }
    #[test]
    fn tile_uses_balanced_columns_but_falls_back_to_stacking_when_needed() {
        let area = Rect::new(0, 1, 132, 38);
        let mixed = plan(area, &[(72, 12), (48, 10), (48, 10)], Arrangement::Tile).unwrap();
        assert_eq!(mixed[0], Rect::new(0, 1, 78, 38));
        assert_eq!(mixed[1], Rect::new(78, 1, 54, 19));
        assert_eq!(mixed[2], Rect::new(78, 20, 54, 19));
        let small = Rect::new(0, 1, 80, 23);
        assert_eq!(
            plan(small, &[(72, 12), (72, 12)], Arrangement::Tile),
            Err(Error::NoRoom)
        );
        assert_eq!(
            plan(small, &[(72, 12), (48, 10)], Arrangement::Tile),
            Ok(vec![Rect::new(0, 1, 80, 12), Rect::new(0, 13, 80, 11)])
        );
    }
    #[test]
    fn cascade_offsets_titles_and_keeps_a_common_bottom_right_corner() {
        let a = Rect::new(2, 3, 100, 30);
        let rects = plan(a, &[(72, 12); 4], Arrangement::Cascade).unwrap();
        for (i, r) in rects.iter().enumerate() {
            assert_eq!(
                *r,
                Rect::new(2 + i as u16, 3 + i as u16, 100 - i as u16, 30 - i as u16)
            );
        }
        assert_eq!(
            plan(
                Rect::new(0, 1, 73, 23),
                &[(72, 12); 3],
                Arrangement::Cascade
            ),
            Err(Error::NoRoom)
        );
    }
    #[test]
    fn empty_tiny_and_single_window_layouts_are_explicit() {
        let a = Rect::new(0, 1, 80, 23);
        for mode in [Arrangement::Tile, Arrangement::Cascade] {
            assert_eq!(plan(a, &[], mode), Err(Error::NoWindows));
            assert_eq!(plan(Rect::default(), &[(1, 1)], mode), Err(Error::NoRoom));
            assert_eq!(plan(a, &[(72, 12)], mode), Ok(vec![a]));
        }
    }
}
