//! Shared GPU instance classification and immutable data sources.
use crate::tracks::{PreparedTracks, TraceInstance};
// Implementations below expose only padding-free, initialized GPU ABI structs.
pub trait InstanceSource: Send + Sync + 'static {
    const COMPACT_TEXT: bool;
    const MSDF: bool = false;
    type Instance: Send + Sync + bytemuck::Pod;
    fn instances(&self) -> &[Self::Instance];
    fn batches(&self) -> &[crate::tracks::TraceBatch];
    fn selection_ids(&self, index: usize) -> [u32; 4];
    fn bounds(&self, _index: usize) -> Option<pomelo_core::model::Bounds> {
        None
    }
    fn font(&self) -> Option<&crate::text::msdf::MsdfFont> {
        None
    }
    fn atlas_page(&self, _: usize) -> u16 {
        0
    }
    fn label_category(&self, _: usize) -> u32 {
        u32::MAX
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        pomelo_core::selection::SelectedObject::Segment(pomelo_core::model::ObjectId(
            self.selection_ids(index)[0],
        ))
    }
}
impl InstanceSource for PreparedTracks {
    const COMPACT_TEXT: bool = false;
    type Instance = TraceInstance;
    fn instances(&self) -> &[TraceInstance] {
        &self.instances
    }
    fn batches(&self) -> &[crate::tracks::TraceBatch] {
        &self.batches
    }
    fn selection_ids(&self, index: usize) -> [u32; 4] {
        self.instances[index].ids
    }
    fn bounds(&self, index: usize) -> Option<pomelo_core::model::Bounds> {
        let instance = &self.instances[index];
        Some(pomelo_core::model::Bounds {
            min: split_point(instance.bounds_min),
            max: split_point(instance.bounds_max),
        })
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        let instance = &self.instances[index];
        let id = pomelo_core::model::ObjectId(instance.ids[0]);
        if instance.flags[3] & 64 != 0 {
            pomelo_core::selection::SelectedObject::Drawing(id)
        } else if instance.flags[3] & 8 != 0 {
            pomelo_core::selection::SelectedObject::Zone(id)
        } else if instance.flags[3] & 16 != 0 {
            pomelo_core::selection::SelectedObject::Pin(id)
        } else if instance.flags[3] & 32 != 0 {
            pomelo_core::selection::SelectedObject::Via(id)
        } else {
            pomelo_core::selection::SelectedObject::Segment(id)
        }
    }
}
impl InstanceSource for crate::text_instances::PreparedTextInstances {
    const COMPACT_TEXT: bool = true;
    type Instance = crate::text_instances::TextInstance;
    fn instances(&self) -> &[Self::Instance] {
        &self.instances
    }
    fn batches(&self) -> &[crate::tracks::TraceBatch] {
        &self.batches
    }
    fn selection_ids(&self, index: usize) -> [u32; 4] {
        [
            if self.instances[index].flags[1] == 1 {
                self.instances[index].flags[0]
            } else {
                self.instances[index].ids[0]
            },
            0,
            self.instances[index].ids[1],
            0,
        ]
    }
    fn bounds(&self, index: usize) -> Option<pomelo_core::model::Bounds> {
        let instance = &self.instances[index];
        let a = split_point(instance.a);
        let b = split_point(instance.b);
        let radius = f64::from(f32::from_bits(instance.ids[3])) * 0.5;
        Some(pomelo_core::model::Bounds {
            min: pomelo_core::model::Point::new(a.x.min(b.x) - radius, a.y.min(b.y) - radius),
            max: pomelo_core::model::Point::new(a.x.max(b.x) + radius, a.y.max(b.y) + radius),
        })
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        let stroke = &self.instances[index];
        pomelo_core::selection::SelectedObject::Drawing(pomelo_core::model::ObjectId(
            if stroke.flags[1] == 1 {
                stroke.flags[0]
            } else {
                u32::MAX
            },
        ))
    }
}

impl InstanceSource for crate::text::msdf::PreparedGlyphs {
    const COMPACT_TEXT: bool = false;
    const MSDF: bool = true;
    type Instance = crate::text::msdf::GlyphInstance;
    fn instances(&self) -> &[Self::Instance] {
        &self.instances
    }
    fn batches(&self) -> &[crate::tracks::TraceBatch] {
        &self.batches
    }
    fn selection_ids(&self, index: usize) -> [u32; 4] {
        let ids = self.instances[index].ids;
        [ids[0], 0, ids[2], ids[3]]
    }
    fn bounds(&self, index: usize) -> Option<pomelo_core::model::Bounds> {
        let glyph = &self.instances[index];
        let x = f64::from(glyph.xywh[0]) + f64::from(glyph.low[0]);
        let y = f64::from(glyph.xywh[1]) + f64::from(glyph.low[1]);
        let [cos, sin, mirror, _] = glyph.rotation.map(f64::from);
        pomelo_core::model::Bounds::from_points(
            [
                (0.0, 0.0),
                (f64::from(glyph.xywh[2]), 0.0),
                (0.0, f64::from(glyph.xywh[3])),
                (f64::from(glyph.xywh[2]), f64::from(glyph.xywh[3])),
            ]
            .map(|(dx, dy)| {
                pomelo_core::model::Point::new(
                    x + (dx * cos - dy * sin) * mirror,
                    y + dx * sin + dy * cos,
                )
            }),
        )
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        pomelo_core::selection::SelectedObject::Drawing(pomelo_core::model::ObjectId(
            if self.instances[index].low[2] == 1.0 {
                self.instances[index].ids[0]
            } else {
                u32::MAX
            },
        ))
    }
    fn font(&self) -> Option<&crate::text::msdf::MsdfFont> {
        Some(&self.font)
    }
    fn atlas_page(&self, index: usize) -> u16 {
        self.instances[index].page()
    }
    fn label_category(&self, index: usize) -> u32 {
        self.instances[index].ids[1]
    }
}

fn split_point(point: [f32; 4]) -> pomelo_core::model::Point {
    pomelo_core::model::Point::new(
        f64::from(point[0]) + f64::from(point[2]),
        f64::from(point[1]) + f64::from(point[3]),
    )
}
