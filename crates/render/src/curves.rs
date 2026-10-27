//! Local read-only checks on original fill curves. No offsets or all-pairs boolean
//! operations. Results are bounded numerical samples, never proof of a whole edge.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use vectorcraft_doc::{Document, Node, NodeId, NodeKind};
use vectorcraft_geom::{BezPath, FillRule, ParamCurve, PathEl, PathSeg, Point, Rect, Shape};

pub const STEP_MM: f64 = 0.05;
pub const PROBE_MM: f64 = 0.0001;
const PT_MM: f64 = 72.0 / 25.4;
const MAX_ELEMENTS: usize = 16_000;

pub struct Verification {
    pub tested: usize,
    pub visible: usize,
    pub occluded: usize,
    pub gaps: usize,
    pub unresolved: usize,
    pub truncated: bool,
    pub fragments: Vec<[f64; 6]>,
    pub fragments_truncated: bool,
    pub local_objects: usize,
}

struct Geometry {
    path: BezPath,
    rule: FillRule,
    bounds: Rect,
}
struct Surface {
    id: NodeId,
    geometry: Arc<Geometry>,
    clips: Vec<Arc<Geometry>>,
}
struct Scene {
    surfaces: Vec<Surface>,
    cache: BTreeMap<NodeId, Arc<Geometry>>,
    elements: usize,
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && a.x1 >= b.x0 && a.y0 <= b.y1 && a.y1 >= b.y0
}

impl Geometry {
    fn contains(&self, p: Point) -> bool {
        self.bounds.contains(p) && vectorcraft_geom::hit::fill_contains(&self.path, self.rule, p)
    }
}

impl Scene {
    fn geometry(&mut self, n: &Node) -> Result<Arc<Geometry>, String> {
        if let Some(g) = self.cache.get(&n.id) {
            return Ok(g.clone());
        }
        let points = match &n.kind {
            NodeKind::Path { path, .. } => path.subpaths.iter().map(|s| s.anchors.len()).sum::<usize>(),
            NodeKind::Compound { children, .. } => {
                children.iter().filter_map(|c| c.path_data()).flat_map(|p| &p.subpaths).map(|s| s.anchors.len()).sum()
            }
            _ => return Err("El contorno requiere expandir su geometría antes de verificarlo.".into()),
        };
        self.elements = self.elements.saturating_add(points.saturating_mul(2));
        if self.elements > MAX_ELEMENTS {
            return Err("La zona supera el presupuesto de curvas. Verifica una región menor.".into());
        }
        let path = n.stroke_path().ok_or("No se pudo leer la curva original.")?;
        let mut closed = BezPath::new();
        let mut open = false;
        for &el in path.elements() {
            let finite = |p: Point| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 1e9 && p.y.abs() <= 1e9;
            let valid = match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => finite(p),
                PathEl::QuadTo(a, b) => finite(a) && finite(b),
                PathEl::CurveTo(a, b, c) => finite(a) && finite(b) && finite(c),
                PathEl::ClosePath => true,
            };
            if !valid {
                return Err("La curva contiene coordenadas no válidas.".into());
            }
            match el {
                PathEl::MoveTo(_) => {
                    if open {
                        closed.close_path();
                    }
                    open = true;
                }
                PathEl::ClosePath => open = false,
                _ => {}
            }
            closed.push(el);
        }
        if open {
            closed.close_path();
        }
        let rule = match n.kind {
            NodeKind::Path { rule, .. } | NodeKind::Compound { rule, .. } => rule,
            _ => FillRule::NonZero,
        };
        let g = Arc::new(Geometry { bounds: closed.bounding_box(), path: closed, rule });
        self.cache.insert(n.id, g.clone());
        Ok(g)
    }

    fn collect(&mut self, n: &Node, region: Rect, clips: &[Arc<Geometry>]) -> Result<(), String> {
        if !n.visible || n.reach_bounds().is_none_or(|b| !overlaps(b, region)) {
            return Ok(());
        }
        if matches!(n.kind, NodeKind::Layer { .. } | NodeKind::Group { .. }) {
            let mut next = clips.to_vec();
            if n.clips()
                && let Some(first) = n.children().and_then(|c| c.first())
            {
                next.push(self.geometry(first)?);
            }
            for c in n.children().into_iter().flatten() {
                self.collect(c, region, &next)?;
            }
        } else if !n.appearance.fill_paint().is_none() {
            if self.surfaces.len() >= 512 {
                return Err("La zona contiene más de 512 superficies. Verifica una región menor.".into());
            }
            let geometry = self.geometry(n)?;
            self.surfaces.push(Surface { id: n.id, geometry, clips: clips.to_vec() });
        }
        Ok(())
    }

    fn owner(&self, p: Point) -> Option<NodeId> {
        self.surfaces.iter().rev().find(|s| s.geometry.contains(p) && s.clips.iter().all(|c| c.contains(p))).map(|s| s.id)
    }
}

/// Checks only the requested pair and region, including original clip contours.
/// Unknown compositing is a conservative blocker inherited from source attribution.
pub fn verify(doc: &Document, ids: [NodeId; 2], region: Rect) -> Result<Verification, String> {
    if ids[0] == ids[1]
        || ![region.x0, region.y0, region.x1, region.y1].iter().all(|v| v.is_finite() && v.abs() <= 1e9)
        || region.width() <= 0.0
        || region.height() <= 0.0
        || region.width() > 100_000.0
        || region.height() > 100_000.0
    {
        return Err("Elige dos objetos distintos y una región finita de tamaño positivo.".into());
    }
    crate::technical::check_art(doc)?;
    let (mapped, sources) = crate::sources::prepare(doc);
    let known: BTreeSet<_> = sources.iter().map(|s| s.id).collect();
    if ids.iter().any(|id| !known.contains(id)) {
        return Err("Este par requiere dos sólidos opacos visibles, sin trazo, máscara ni sobreimpresión.".into());
    }
    let mut scene = Scene { surfaces: Vec::new(), cache: BTreeMap::new(), elements: 0 };
    for n in &mapped.layers {
        scene.collect(n, region.inflate(0.01, 0.01), &[])?;
    }
    let mut candidates = BTreeSet::new();
    for s in &scene.surfaces {
        if ids.contains(&s.id) {
            candidates.insert(s.id);
            for clip in &s.clips {
                for (id, g) in &scene.cache {
                    if Arc::ptr_eq(clip, g) {
                        candidates.insert(*id);
                    }
                }
            }
        }
    }
    let mut work = Vec::<(PathSeg, u8)>::new();
    for id in candidates {
        if let Some(g) = scene.cache.get(&id) {
            for segment in g.path.segments().filter(|s| overlaps(s.bounding_box(), region)) {
                work.push((segment, 0));
            }
        }
    }
    // Bound point-in-fill work as well as the number of samples. Clips count each
    // time they are tested, not merely once in the shared geometry cache.
    let cost = scene
        .surfaces
        .iter()
        .map(|s| s.geometry.path.elements().len() + s.clips.iter().map(|g| g.path.elements().len()).sum::<usize>())
        .sum::<usize>();
    let max_samples = (4_000_000 / cost.max(1).saturating_mul(4)).clamp(1, 8192);
    let mut r = Verification {
        tested: 0,
        visible: 0,
        occluded: 0,
        gaps: 0,
        unresolved: 0,
        truncated: false,
        fragments: Vec::new(),
        fragments_truncated: false,
        local_objects: scene.surfaces.len(),
    };
    let mut visits = 0;
    while let Some((segment, depth)) = work.pop() {
        visits += 1;
        if visits > 60_000 || r.tested >= max_samples {
            r.truncated = true;
            break;
        }
        let bounds = segment.bounding_box();
        if !overlaps(bounds, region) {
            continue;
        }
        if bounds.width().hypot(bounds.height()) > STEP_MM * PT_MM && depth < 24 {
            work.push((segment.subsegment(0.5..1.0), depth + 1));
            work.push((segment.subsegment(0.0..0.5), depth + 1));
            continue;
        }
        if depth == 24 {
            r.truncated = true;
            continue;
        }
        let p = segment.eval(0.5);
        if !region.contains(p) {
            continue;
        }
        r.tested += 1;
        let tangent = segment.eval(0.5001) - segment.eval(0.4999);
        let len = tangent.hypot();
        if len < 1e-12 {
            r.unresolved += 1;
            continue;
        }
        let normal = vectorcraft_geom::Vec2::new(-tangent.y, tangent.x) / len;
        let owners = |mm| [scene.owner(p + normal * (mm * PT_MM)), scene.owner(p - normal * (mm * PT_MM))];
        let near = owners(PROBE_MM);
        let far = owners(0.002);
        if near.iter().chain(far.iter()).flatten().any(|id| !known.contains(id)) {
            r.unresolved += 1;
            continue;
        }
        let pair = near[0].zip(near[1]).is_some_and(|(a, b)| a != b && ids.contains(&a) && ids.contains(&b));
        if pair && near == far {
            r.visible += 1;
            let a = segment.eval(0.0);
            let b = segment.eval(1.0);
            r.fragments.push([a.x, a.y, p.x, p.y, b.x, b.y]);
        } else if near.iter().any(Option::is_none) {
            r.gaps += 1;
        } else if near != far {
            r.unresolved += 1;
        } else {
            r.occluded += 1;
        }
    }
    if r.fragments.len() > 1024 {
        let count = r.fragments.len();
        r.fragments = (0..1024).filter_map(|i| r.fragments.get(i * count / 1024).copied()).collect();
        r.fragments_truncated = true;
    }
    Ok(r)
}
