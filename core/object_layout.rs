use crate::*;
use std::collections::BTreeSet;

/// Alignment uses the outer selection bounds; distribution uses equal edge gaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectLayout {
    AlignLeft,
    AlignRight,
    AlignTop,
    AlignBottom,
    AlignHorizontalCenter,
    AlignVerticalCenter,
    DistributeHorizontal,
    DistributeVertical,
}

impl Engine {
    /// Align or distribute same-page objects atomically, preserving their contents.
    pub fn layout_page_items(
        &self,
        doc: &mut Document,
        items: &[PageItem],
        layout: ObjectLayout,
    ) -> Result<()> {
        guarded(|| {
            let first = items
                .first()
                .ok_or_else(|| err("Select at least two objects"))?;
            let distribute = matches!(
                layout,
                ObjectLayout::DistributeHorizontal | ObjectLayout::DistributeVertical
            );
            if items.len() < if distribute { 3 } else { 2 } || items.len() > 20_000 {
                return Err(err(if distribute {
                    "Select at least three objects to distribute"
                } else {
                    "Select at least two objects to align"
                }));
            }
            if items
                .iter()
                .any(|i| i.page != first.page || i.revision != doc.revision)
            {
                return Err(err("Selection is stale or spans pages"));
            }
            let ids: BTreeSet<_> = items.iter().map(|i| i.index).collect();
            if ids.len() != items.len() {
                return Err(err("Duplicate object selection"));
            }
            let current = self.page_items(doc, first.page)?;
            let mut selected = items
                .iter()
                .map(|i| {
                    current
                        .iter()
                        .find(|c| c.index == i.index)
                        .cloned()
                        .ok_or_else(|| err("Object no longer exists"))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut outer = selected[0].bounds;
            for item in &selected {
                outer.left = outer.left.min(item.bounds.left);
                outer.right = outer.right.max(item.bounds.right);
                outer.bottom = outer.bottom.min(item.bounds.bottom);
                outer.top = outer.top.max(item.bounds.top);
            }
            let horizontal = layout == ObjectLayout::DistributeHorizontal;
            let mut next = if horizontal { outer.left } else { outer.bottom };
            let mut gap = 0.0;
            if distribute {
                selected.sort_by(|a, b| {
                    let (a, b) = if horizontal {
                        (a.bounds.left, b.bounds.left)
                    } else {
                        (a.bounds.bottom, b.bounds.bottom)
                    };
                    a.total_cmp(&b)
                });
                let extent = if horizontal {
                    outer.right - outer.left
                } else {
                    outer.top - outer.bottom
                };
                let sizes: f32 = selected
                    .iter()
                    .map(|i| {
                        if horizontal {
                            i.bounds.right - i.bounds.left
                        } else {
                            i.bounds.top - i.bounds.bottom
                        }
                    })
                    .sum();
                gap = (extent - sizes) / (selected.len() - 1) as f32;
            }
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            let mut changed = false;
            let total = selected.len();
            for (index, item) in selected.into_iter().enumerate() {
                operation::checkpoint("Arranging objects", index, total)?;
                let b = item.bounds;
                let (dx, dy) = match layout {
                    ObjectLayout::AlignLeft => (outer.left - b.left, 0.0),
                    ObjectLayout::AlignRight => (outer.right - b.right, 0.0),
                    ObjectLayout::AlignTop => (0.0, outer.top - b.top),
                    ObjectLayout::AlignBottom => (0.0, outer.bottom - b.bottom),
                    ObjectLayout::AlignHorizontalCenter => {
                        ((outer.left + outer.right - b.left - b.right) * 0.5, 0.0)
                    }
                    ObjectLayout::AlignVerticalCenter => {
                        (0.0, (outer.bottom + outer.top - b.bottom - b.top) * 0.5)
                    }
                    ObjectLayout::DistributeHorizontal => {
                        let d = next - b.left;
                        next += b.right - b.left + gap;
                        (d, 0.0)
                    }
                    ObjectLayout::DistributeVertical => {
                        let d = next - b.bottom;
                        next += b.top - b.bottom + gap;
                        (0.0, d)
                    }
                };
                if dx.abs() < 0.0001 && dy.abs() < 0.0001 {
                    continue;
                }
                let fresh = self
                    .page_items(&candidate, first.page)?
                    .into_iter()
                    .find(|i| i.index == item.index)
                    .ok_or_else(|| {
                        err("Object layout changed the selection; document unchanged")
                    })?;
                self.edit_page_items(&mut candidate, &[fresh], ObjectAction::Move { dx, dy })?;
                candidate.undo.clear();
                changed = true;
            }
            if changed {
                operation::checkpoint("Applying object layout", total, total)?;
                doc.accept(candidate.bytes);
            }
            Ok(())
        })
    }
}
