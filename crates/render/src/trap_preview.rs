//! Experimental local band preview. Original Bézier intervals supply the centreline;
//! sampled visibility gates each interval. Nothing is inserted in the user's document.
use crate::curves::{Geometry, Scene, local_scene, overlaps};
use std::collections::BTreeSet;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, Node, NodeId};
use vectorcraft_geom::{BezPath, FillRule, ParamCurve, PathData, PathSeg, Point, Rect, Shape, Vec2, kurbo};
use vectorcraft_pathops::{BoolOp, try_boolean, try_normalize};

const PT_MM: f64 = 72.0 / 25.4;
const PRECISION: f64 = 0.0001;
const MAX_GEOMETRY: usize = 4096;

pub struct Preview {
    pub band: PathData,
    pub runs: Vec<BezPath>,
    pub tested: usize,
    pub region: Rect,
    pub dpi: f64,
    pub base: crate::Rendered,
    pub overlay: crate::Rendered,
    pub local_objects: usize,
}

fn bounded(path: PathData) -> Result<PathData, String> {
    if path.subpaths.iter().map(|s| s.anchors.len()).sum::<usize>() > MAX_GEOMETRY {
        return Err("La franja supera el presupuesto de geometría. Revisa una zona menor.".into());
    }
    Ok(path)
}
fn combine(a: &PathData, b: &PathData, op: BoolOp) -> Result<PathData, String> {
    bounded(try_boolean(a, FillRule::NonZero, b, FillRule::NonZero, op, PRECISION).map_err(|e| format!("No se pudo recortar la franja: {e}"))?)
}
fn normalized(g: &Geometry) -> Result<PathData, String> {
    bounded(try_normalize(&PathData::from_bezpath(&g.path), g.rule).map_err(|e| format!("No se pudo resolver un contorno: {e}"))?)
}
fn matches_pair(scene: &Scene, ids: [NodeId; 2], segment: PathSeg, t: f64) -> bool {
    let p = segment.eval(t);
    let tangent = segment.eval((t + 0.00001).min(1.0)) - segment.eval((t - 0.00001).max(0.0));
    let len = tangent.hypot();
    if len < 1e-12 {
        return false;
    }
    let n = Vec2::new(-tangent.y, tangent.x) / len;
    let owners = |mm| [scene.owner(p + n * (mm * PT_MM)), scene.owner(p - n * (mm * PT_MM))];
    let near = owners(0.0001);
    near == owners(0.002) && near[0].zip(near[1]).is_some_and(|(a, b)| a != b && ids.contains(&a) && ids.contains(&b))
}

/// Rejoin adjacent parameter intervals of each original curve before stroking.
/// Five checks per small interval reduce leaks at transitions; they remain numerical
/// samples, not a continuous contact proof. Rejected pieces always break a run.
fn trace(scene: &Scene, ids: [NodeId; 2], region: Rect) -> Result<(Vec<BezPath>, usize), String> {
    let mut candidates = BTreeSet::new();
    for surface in &scene.surfaces {
        if ids.contains(&surface.id) {
            candidates.insert(surface.id);
            for (id, geometry) in &scene.cache {
                if surface.clips.iter().any(|clip| std::sync::Arc::ptr_eq(clip, geometry)) {
                    candidates.insert(*id);
                }
            }
        }
    }
    let cost = scene
        .surfaces
        .iter()
        .map(|s| s.geometry.path.elements().len() + s.clips.iter().map(|g| g.path.elements().len()).sum::<usize>())
        .sum::<usize>();
    let max_cells = (8_000_000 / cost.max(1).saturating_mul(20)).clamp(1, 8192);
    let mut runs = Vec::new();
    let mut tested = 0;
    let mut visits = 0;
    for id in candidates {
        let Some(g) = scene.cache.get(&id) else { continue };
        let mut current = BezPath::new();
        let mut last = None::<Point>;
        for original in g.path.segments() {
            let mut work = vec![(0.0, 1.0, 0u8)];
            let mut ranges: Vec<(f64, f64)> = Vec::new();
            while let Some((lo, hi, depth)) = work.pop() {
                visits += 1;
                if visits > 60_000 || tested >= max_cells {
                    return Err("La previsualización alcanzó su presupuesto. Elige una zona menor.".into());
                }
                let seg = original.subsegment(lo..hi);
                let b = seg.bounding_box();
                if !overlaps(b, region) {
                    continue;
                }
                if b.width().hypot(b.height()) > 0.05 * PT_MM {
                    if depth >= 24 {
                        return Err("El contorno requiere una zona menor para previsualizarse.".into());
                    }
                    let mid = (lo + hi) * 0.5;
                    work.push((mid, hi, depth + 1));
                    work.push((lo, mid, depth + 1));
                    continue;
                }
                tested += 1;
                let accepted = [0.02, 0.25, 0.5, 0.75, 0.98].iter().all(|&t| region.contains(seg.eval(t)) && matches_pair(scene, ids, seg, t));
                if accepted {
                    if let Some(previous) = ranges.last_mut().filter(|p| (p.1 - lo).abs() < 1e-12) {
                        previous.1 = hi;
                    } else {
                        ranges.push((lo, hi));
                    }
                }
            }
            for (lo, hi) in ranges {
                let seg = original.subsegment(lo..hi);
                let start = seg.eval(0.0);
                if last.is_none_or(|p| p.distance(start) > 1e-8) {
                    if !current.elements().is_empty() {
                        runs.push(std::mem::take(&mut current));
                    }
                    current.move_to(start);
                }
                match seg {
                    PathSeg::Line(l) => current.line_to(l.p1),
                    PathSeg::Quad(q) => current.quad_to(q.p1, q.p2),
                    PathSeg::Cubic(c) => current.curve_to(c.p1, c.p2, c.p3),
                }
                last = Some(seg.eval(1.0));
            }
        }
        if !current.elements().is_empty() {
            runs.push(current);
        }
    }
    if runs.len() > 256 {
        return Err("Demasiados tramos separados. Revisa una zona menor.".into());
    }
    // Closing a full loop gives the stroker a genuine corner join at its seam.
    for run in &mut runs {
        let endpoints = run.segments().next().zip(run.segments().last());
        if endpoints.is_some_and(|(start, end)| start.eval(0.0).distance(end.eval(1.0)) < 1e-8) {
            run.close_path();
        }
    }
    Ok((runs, tested))
}

/// Ordered IDs define source → receiver. The band is highlighted geometry, not a
/// prediction of overprint colour. It is clipped to the receiver's visible fill.
pub fn preview(doc: &Document, ids: [NodeId; 2], region: Rect, width_mm: f64) -> Result<Preview, String> {
    if !width_mm.is_finite() || !(0.01..=0.5).contains(&width_mm) {
        return Err("El ancho de previsualización debe estar entre 0,01 y 0,50 mm.".into());
    }
    // Validate the requested region before inflating it (a reversed region must fail).
    crate::curves::validate_region(ids, region)?;
    let region = region.inflate(width_mm * PT_MM + 0.1, width_mm * PT_MM + 0.1);
    let (scene, known) = local_scene(doc, ids, region)?;
    if scene.surfaces.len() > 32 || scene.elements > 2048 {
        return Err("La previsualización local admite 32 superficies y 1.024 anclas. Revisa una zona menor.".into());
    }
    let (runs, tested) = trace(&scene, ids, region)?;
    if runs.is_empty() {
        return Err("No se encontraron tramos continuos verificables para esta franja.".into());
    }
    let mut receiver = PathData::default();
    let mut local = doc.clone();
    local.layers.clear();
    local.symbols.clear();
    local.images.clear();
    let layer = local.default_layer();
    for surface in &scene.surfaces {
        let mut shape = normalized(&surface.geometry)?;
        for clip in &surface.clips {
            shape = combine(&shape, &normalized(clip)?, BoolOp::Intersect)?;
        }
        // An unknown blocker is removed as well: never spread into unsupported art.
        receiver = combine(&receiver, &shape, BoolOp::Difference)?;
        if surface.id == ids[1] {
            receiver = combine(&receiver, &shape, BoolOp::Union)?;
        }
        let paint = if known.contains(&surface.id) {
            doc.node(surface.id)
                .and_then(|n| {
                    n.appearance.items.iter().find(|i| i.visible() && i.opacity() > 0.0 && i.is_fill() && !matches!(i.paint(), Paint::None))
                })
                .map(|i| i.paint().clone())
                .unwrap_or(Paint::None)
        } else {
            Paint::solid(Color::rgb(0.5, 0.5, 0.5))
        };
        let id = local.alloc_id();
        local.insert(layer, usize::MAX, Node::path(id, shape, Appearance::basic(paint, Paint::None, 0.0))).map_err(|e| e.to_string())?;
    }
    let style = kurbo::Stroke::new(width_mm * PT_MM * 2.0).with_caps(kurbo::Cap::Butt).with_join(kurbo::Join::Round);
    let mut band = PathData::default();
    for run in &runs {
        let mut outline = kurbo::stroke(run.iter(), &style, &kurbo::StrokeOpts::default(), 0.0001);
        outline.close_path();
        let outline = bounded(try_normalize(&PathData::from_bezpath(&outline), FillRule::NonZero).map_err(|e| e.to_string())?)?;
        band = combine(&band, &outline, BoolOp::Union)?;
    }
    band = combine(&band, &receiver, BoolOp::Intersect)?;
    band = combine(&band, &vectorcraft_geom::shapes::rectangle(region), BoolOp::Intersect)?;
    if band.is_empty() {
        return Err("La franja queda vacía al recortarla al receptor visible.".into());
    }
    // Bound both dimensions and total pixels, even for very thin regions.
    let scale = (600.0_f64 / 72.0).min(1000.0 / region.width()).min(1000.0 / region.height());
    crate::raster_size(region, scale)?;
    let mut renderer = crate::Renderer::new();
    let base = renderer.render_region(&local, region, scale, true);
    local.layers.clear();
    let layer = local.default_layer();
    let id = local.alloc_id();
    local
        .insert(layer, usize::MAX, Node::path(id, band.clone(), Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0)))
        .map_err(|e| e.to_string())?;
    let overlay = renderer.render_region(&local, region, scale, false);
    Ok(Preview { band, runs, tested, region, dpi: scale * 72.0, base, overlay, local_objects: scene.surfaces.len() })
}
