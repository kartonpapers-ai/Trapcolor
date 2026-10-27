//! Conservative source attribution for sampled ink boundaries. One hard-edge ID
//! render, then bounded probes from actual boundary pixels: no all-pairs geometry.
//! Unsupported artwork blocks attribution instead of revealing hidden objects.

use crate::{AntiAlias, RenderOptions, Renderer, boundaries::Analysis};
use std::collections::BTreeMap;
use std::sync::Arc;
use vectorcraft_color::{BlendMode, Color, Paint};
use vectorcraft_doc::{Appearance, AppearanceItem, ColorMode, Document, Knockout, Node, NodeId, NodeKind};
use vectorcraft_geom::{Rect, shapes};

pub struct Source {
    pub id: NodeId,
    pub name: String,
    pub bounds: Rect,
    pub plates: Vec<String>,
}

pub struct Pair {
    pub left: usize,
    pub right: usize,
    pub samples: usize,
}

pub struct ZoneSources {
    pub pairs: Vec<Pair>,
    pub unresolved: usize,
    pub sampled: usize,
    pub truncated: bool,
}

pub struct Attribution {
    pub sources: Vec<Source>,
    pub zones: Vec<ZoneSources>,
}

fn code(index: usize) -> [u8; 3] {
    let x = index as u32;
    // Nonlinear check byte guards against colour conversion/edge contamination.
    [x as u8, (x >> 8) as u8, ((x.wrapping_mul(x + 17) ^ 0xa7) & 255) as u8]
}

fn paint(index: usize) -> Paint {
    let [r, g, b] = code(index).map(|v| f32::from(v) / 255.0);
    // Lab uses the renderer's analytic display conversion, bypassing working RGB
    // ICC gamut clipping. Auxiliary IDs must not become another object's colour.
    let lab = vectorcraft_color::cms::lab::srgb_to_lab([r, g, b]);
    Paint::solid(Color::lab(lab.l, lab.a, lab.b))
}

fn active(item: &AppearanceItem) -> bool {
    item.visible() && item.opacity() > 0.0 && !matches!(item.paint(), Paint::None) && !matches!(item, AppearanceItem::Stroke(s) if s.width <= 0.0)
}

fn simple(n: &Node) -> bool {
    n.opacity == 1.0
        && n.blend == BlendMode::Normal
        && n.knockout != Knockout::On
        && n.extras().mask.is_none()
        && n.extras().shaper.is_none()
        && n.appearance.effects.is_empty()
}

fn eligible(n: &Node) -> bool {
    if !simple(n) || !matches!(n.kind, NodeKind::Path { guide: false, .. } | NodeKind::Compound { .. }) {
        return false;
    }
    if n.extras().attrs.as_ref().is_some_and(|a| a.data.iter().any(|(k, _)| k.starts_with("kx27-trap-v1-"))) {
        return false;
    }
    let mut items = n.appearance.items.iter().filter(|i| active(i));
    matches!(items.next(), Some(AppearanceItem::Fill(f)) if f.opacity == 1.0
        && f.blend == BlendMode::Normal && !f.overprint && f.effects.is_empty()
        && matches!(f.paint, Paint::Solid { .. }))
        && items.next().is_none()
}

fn transform(arc: &mut Arc<Node>, doc: &Document, sources: &mut Vec<Source>, plates: &[crate::proof::Plate]) {
    let n = Arc::make_mut(arc);
    if !n.visible
        || n.opacity == 0.0
        || matches!(n.kind, NodeKind::Layer { template: true, .. } | NodeKind::Layer { printable: false, .. } | NodeKind::Path { guide: true, .. })
    {
        n.visible = false;
        return;
    }
    // Never replace an unsupported clipping path by its bounds and accidentally
    // expose clipped-away artwork. Block that whole container conservatively.
    let safe_clip = !n.clips()
        || n.children().and_then(|c| c.first()).is_some_and(|c| {
            simple(c)
                && matches!(c.kind, NodeKind::Path { guide: false, .. } | NodeKind::Compound { .. })
                && (eligible(c) || !c.appearance.items.iter().any(active))
        });
    if matches!(n.kind, NodeKind::Layer { .. } | NodeKind::Group { .. }) && simple(n) && safe_clip && !n.appearance.items.iter().any(active) {
        if let NodeKind::Layer { preview, dim_images, .. } = &mut n.kind {
            *preview = true;
            *dim_images = None;
        }
        for child in n.children_mut().into_iter().flatten() {
            transform(child, doc, sources, plates);
        }
        return;
    }
    if eligible(n)
        && let Some(bounds) = n.geometric_bounds()
    {
        let cms = vectorcraft_color::cms::active();
        let plates = n
            .appearance
            .items
            .iter()
            .filter(|i| active(i))
            .find_map(|i| {
                if let Paint::Solid { color, swatch, tint } = i.paint() {
                    let inks = vectorcraft_doc::inks::inks(doc, &cms, color, swatch.as_deref().map(|s| (s, *tint)), cms.settings().intent);
                    Some(plates.iter().filter(|p| inks.on_plate(&p.name) > 0.0).map(|p| p.name.clone()).collect())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        sources.push(Source { id: n.id, name: n.name.clone().unwrap_or_else(|| format!("Objeto {}", n.id.0)), bounds, plates });
        n.appearance = Appearance::basic(paint(sources.len()), Paint::None, 0.0).into();
        return;
    }
    // Empty vector paths paint nothing but can still supply a clipping outline.
    if matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) && !n.appearance.items.iter().any(active) {
        return;
    }
    // Conservatively block the entire bounds for images, strokes, masks, group
    // compositing, meshes, gradients and overprint. Never see through them in the ID map.
    if let Some(bounds) = n.reach_bounds() {
        let mut blocked = Node::path(n.id, shapes::rectangle(bounds), Appearance::basic(paint(0), Paint::None, 0.0));
        blocked.name = n.name.clone();
        *n = blocked;
    } else {
        n.visible = false;
    }
}

/// Temporary conservative surface scene, shared by ID rendering and curve queries.
pub(crate) fn prepare(doc: &Document) -> (Document, Vec<Source>) {
    let mut mapped = doc.clone();
    mapped.color_mode = ColorMode::Rgb;
    let mut sources = Vec::new();
    let plates = crate::proof::plates(doc);
    for node in &mut mapped.layers {
        transform(node, doc, &mut sources, &plates);
    }
    (mapped, sources)
}

/// Call only after the technical capture has validated the original tree and its
/// budgets. The ID map is temporary, not document art and never a colour separation.
pub(crate) fn attribute(renderer: &mut Renderer, doc: &Document, region: Rect, dpi: f64, analysis: &Analysis) -> Attribution {
    if analysis.zones.is_empty() {
        return Attribution { sources: Vec::new(), zones: Vec::new() };
    }
    let (mapped, sources) = prepare(doc);
    let options = RenderOptions {
        background: None,
        anti_alias: AntiAlias::None,
        skip_templates: true,
        dim_templates: false,
        precise: true,
        ..Default::default()
    };
    let map = renderer.render_region_with(&mapped, region, dpi / 72.0, &options);
    let lookup: BTreeMap<_, _> = (1..=sources.len()).map(|i| (code(i), i - 1)).collect();
    let id = |pos: usize| -> Option<usize> {
        let p = map.pixels.get(pos.checked_mul(4)?..pos.checked_mul(4)?.checked_add(4)?)?;
        if p.get(3) != Some(&255) {
            return None;
        }
        lookup.get(&[p[0], p[1], p[2]]).copied()
    };
    let zones = analysis
        .zones
        .iter()
        .map(|zone| {
            let (mut pairs, mut unresolved, mut truncated) = (BTreeMap::<(usize, usize), usize>::new(), 0, false);
            for probe in &zone.probes {
                let ids = probe.positions.map(&id);
                let Some(a) = ids[0] else {
                    unresolved += 1;
                    continue;
                };
                let Some(b) = ids[4] else {
                    unresolved += 1;
                    continue;
                };
                if a == b || ids.iter().any(|v| *v != Some(a) && *v != Some(b)) {
                    unresolved += 1;
                    continue;
                }
                let key = if a < b { (a, b) } else { (b, a) };
                if pairs.len() >= 64 && !pairs.contains_key(&key) {
                    truncated = true;
                    unresolved += 1;
                    continue;
                }
                *pairs.entry(key).or_default() += 1;
            }
            ZoneSources {
                pairs: pairs.into_iter().map(|((left, right), samples)| Pair { left, right, samples }).collect(),
                unresolved,
                sampled: zone.probes.len(),
                truncated,
            }
        })
        .collect();
    Attribution { sources, zones }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_auxiliary_id_roundtrips_through_display_conversion_without_collisions() {
        let mut seen = std::collections::BTreeSet::new();
        for index in 0..=20_000 {
            let color = paint(index).color().unwrap();
            let rgb = color.to_rgba8(1.0);
            let expected = code(index);
            assert_eq!([rgb[0], rgb[1], rgb[2]], expected, "ID {index}");
            assert!(seen.insert(expected));
        }
    }
}
