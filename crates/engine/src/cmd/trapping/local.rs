//! Commit exactly one bounded band prepared by the preceding local preview.
//! The opaque session ticket never accepts geometry supplied by the caller.
use super::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};
use vectorcraft_doc::{Knockout, LayerColor};
use vectorcraft_geom::PathData;

const COMMAND: &str = "trapping.incorporateLocal";
const TAG: &str = "kx27-local-trap-v1";
const LAYER_TAG: &str = "kx27-local-trap-layer-v1";

pub(crate) struct PreparedLocalTrap {
    ticket: u64,
    uid: u64,
    document: Weak<Document>,
    colors: vectorcraft_color::cms::ColorSettings,
    band: PathData,
    paint: Paint,
    record: Record,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Record {
    pub source: NodeId,
    pub receiver: NodeId,
    pub width_mm: f64,
    pub region: [f64; 4],
    pub source_name: String,
    pub receiver_name: String,
}

fn attribute<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.extras().attrs.as_ref()?.data.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}
pub(super) fn is_local(n: &Node) -> bool {
    attribute(n, TAG).is_some()
}
pub(super) fn record(n: &Node) -> Option<Record> {
    let raw = attribute(n, TAG).filter(|v| v.len() <= 4096)?;
    let r: Record = serde_json::from_str(raw).ok()?;
    let [x0, y0, x1, y1] = r.region;
    (r.source != r.receiver
        && r.width_mm.is_finite()
        && (0.01..=0.5).contains(&r.width_mm)
        && r.region.iter().all(|v| v.is_finite() && v.abs() <= 1e9)
        && x1 > x0
        && y1 > y0)
        .then_some(r)
}
fn ink_name(doc: &Document, paint: &Paint) -> Result<String> {
    let encoded = encoded_color(doc, paint)?;
    let color = vectorcraft_trapping::Color::leer(&encoded).ok_or_else(|| bad(COMMAND, "No se pudo identificar la tinta."))?;
    let key = vectorcraft_trapping::etiqueta(&color);
    Ok(display_label(&color, &key).chars().take(90).collect())
}

pub(super) fn remember(s: &mut Session, ids: [NodeId; 2], region: Rect, width_mm: f64, band: PathData, paint: Paint) -> Result<u64> {
    let st = s.doc()?;
    let receiver = st
        .doc
        .node(ids[1])
        .ok_or(EngineError::NoNode(ids[1]))?
        .appearance
        .items
        .iter()
        .find(|i| i.visible() && i.opacity() > 0.0 && i.is_fill() && !i.paint().is_none())
        .map(|i| i.paint().clone())
        .ok_or_else(|| bad(COMMAND, "No se pudo leer la tinta receptora."))?;
    let record = Record {
        source: ids[0],
        receiver: ids[1],
        width_mm,
        region: [region.x0, region.y0, region.x1, region.y1],
        source_name: ink_name(&st.doc, &paint)?,
        receiver_name: ink_name(&st.doc, &receiver)?,
    };
    let ticket = s.local_trap_sequence.checked_add(1).ok_or_else(|| bad(COMMAND, "Reinicia la sesión para crear otra previsualización."))?;
    let prepared = PreparedLocalTrap {
        ticket,
        uid: st.uid,
        document: Arc::downgrade(&st.doc),
        colors: vectorcraft_color::cms::active_settings(),
        band,
        paint,
        record,
    };
    s.local_trap_sequence = ticket;
    s.local_trap_preview = Some(Arc::new(prepared));
    Ok(ticket)
}

// Preserve the source paint exactly. A process source that would lower a shared
// plate needs a separate ink-merge rule; never silently lighten the receiver.
fn validate_print_inks(doc: &Document, source: &Paint, receiver: &Paint) -> Result<()> {
    let cms = vectorcraft_color::cms::active();
    let separate = |paint: &Paint| -> Result<vectorcraft_doc::inks::Inks> {
        let Paint::Solid { color, swatch, tint } = paint else { return Err(bad(COMMAND, "Usa dos rellenos sólidos.")) };
        let spot = swatch.as_ref().is_some_and(|name| doc.swatch(name).is_some_and(|s| s.spot));
        let resolved = swatch.as_ref().and_then(|name| doc.global_color(name)).unwrap_or(*color);
        if !spot && !matches!(resolved, Color::Cmyk { .. }) {
            return Err(bad(
                COMMAND,
                "La incorporación inicial requiere CMYK o tintas spot vinculadas. Convierte los colores RGB/Gris/Lab del par y vuelve a previsualizar.",
            ));
        }
        let resolved = if swatch.is_some() && !spot { resolved.tinted(*tint) } else { resolved };
        Ok(vectorcraft_doc::inks::inks(doc, &cms, &resolved, swatch.as_deref().map(|name| (name, *tint)), cms.settings().intent))
    };
    let a = separate(source)?;
    let b = separate(receiver)?;
    if !a.cmyk.iter().any(|&v| v > 0.0) && !a.spot.as_ref().is_some_and(|(_, tint)| *tint > 0.0) {
        return Err(bad(COMMAND, "La tinta de origen no aporta cobertura de impresión."));
    }
    let lowers = |from: f32, to: f32| from > 0.0 && from + 1e-6 < to;
    let shared_process = a.cmyk.iter().zip(&b.cmyk).any(|(&from, &to)| lowers(from, to));
    let shared_spot =
        a.spot.as_ref().zip(b.spot.as_ref()).is_some_and(|((from_name, from), (to_name, to))| from_name == to_name && lowers(*from, *to));
    if shared_process || shared_spot {
        return Err(bad(
            COMMAND,
            "Este origen reduciría una tinta compartida del receptor. Revisa la dirección; la mezcla de coberturas compartidas requiere una regla posterior.",
        ));
    }
    Ok(())
}

fn managed_layer(doc: &Document) -> Result<Option<NodeId>> {
    let Some((index, layer)) = doc.layers.iter().enumerate().find(|(_, n)| attribute(n, LAYER_TAG) == Some("1")) else { return Ok(None) };
    let ordinary = matches!(layer.kind, NodeKind::Layer { template: false, printable: true, clip: false, preview: true, .. });
    if index + 1 != doc.layers.len()
        || !ordinary
        || !layer.visible
        || layer.locked
        || layer.opacity != 1.0
        || layer.blend != BlendMode::Normal
        || layer.isolate
        || layer.knockout == Knockout::On
        || layer.extras().mask.is_some()
        || layer.extras().shaper.is_some()
        || !layer.appearance.effects.is_empty()
        || layer.appearance.items.iter().any(|i| i.visible() && i.opacity() > 0.0 && !i.paint().is_none())
    {
        return Err(bad(
            COMMAND,
            "La capa de trapping debe estar arriba, visible, desbloqueada, imprimible y sin efectos, recorte ni transparencia. Corrígela y vuelve a previsualizar.",
        ));
    }
    Ok(Some(layer.id))
}
fn overlaps_record(a: &Record, b: &Record) -> bool {
    let pair = (a.source == b.source && a.receiver == b.receiver) || (a.source == b.receiver && a.receiver == b.source);
    let [ax0, ay0, ax1, ay1] = a.region;
    let [bx0, by0, bx1, by1] = b.region;
    pair && ax1.min(bx1) - ax0.max(bx0) > 1e-8 && ay1.min(by1) - ay0.max(by0) > 1e-8
}

pub(super) fn incorporate(s: &mut Session, p: &Value) -> Result<Value> {
    if p.as_object().is_none_or(|o| o.keys().any(|k| k != "previewId")) {
        return Err(bad(COMMAND, "Solo se admite previewId; la geometría procede de la previsualización vigente."));
    }
    let ticket = p["previewId"].as_u64().ok_or_else(|| bad(COMMAND, "Genera primero una previsualización vigente."))?;
    let prepared = s
        .local_trap_preview
        .clone()
        .filter(|v| v.ticket == ticket)
        .ok_or_else(|| bad(COMMAND, "Esta previsualización ya no está disponible. Genera una nueva."))?;
    let st = s.doc()?;
    if st.uid != prepared.uid
        || prepared.document.upgrade().is_none_or(|doc| !Arc::ptr_eq(&doc, &st.doc))
        || prepared.colors != vectorcraft_color::cms::active_settings()
    {
        return Err(bad(COMMAND, "El documento o sus ajustes de color cambiaron. Vuelve a previsualizar antes de incorporar."));
    }
    if st.interaction.is_some() || st.undo_group.is_some() || st.isolation.is_some() || st.doc.mask_edit.is_some() {
        return Err(bad(COMMAND, "Termina la edición activa y sal de aislamiento o edición de máscara antes de incorporar la franja."));
    }
    let receiver = st
        .doc
        .node(prepared.record.receiver)
        .ok_or(EngineError::NoNode(prepared.record.receiver))?
        .appearance
        .items
        .iter()
        .find(|i| i.visible() && i.opacity() > 0.0 && i.is_fill() && !i.paint().is_none())
        .map(|i| i.paint())
        .ok_or_else(|| bad(COMMAND, "No se pudo leer la tinta receptora."))?;
    validate_print_inks(&st.doc, &prepared.paint, receiver)?;
    let mut duplicate = false;
    for n in &st.doc.layers {
        n.walk(&mut |n| {
            if record(n).is_some_and(|r| overlaps_record(&r, &prepared.record)) {
                duplicate = true;
            }
        });
    }
    if duplicate {
        return Err(bad(COMMAND, "Ya existe una franja local para este par en esta zona. Deshaz o elimina la anterior antes de incorporar otra."));
    }
    let layer = managed_layer(&st.doc)?;
    let metadata = serde_json::to_string(&prepared.record).map_err(|e| bad(COMMAND, e.to_string()))?;
    let name = format!("Trap local: {} → {} ({:.2} mm)", prepared.record.source_name, prepared.record.receiver_name, prepared.record.width_mm);
    let (trap_id, layer_id) = s.edit("Incorporar franja de trapping", |d, sel| {
        let layer_id = match layer {
            Some(id) => id,
            None => {
                let id = d.alloc_id();
                let mut layer = Node::layer(id, "KX27 · Trapping local", LayerColor::Preset(0));
                layer.edit_attrs(|a| a.data.push((LAYER_TAG.into(), "1".into())));
                d.insert(None, usize::MAX, layer)?;
                id
            }
        };
        let id = d.alloc_id();
        let mut fill = FillLayer::new(prepared.paint.clone());
        fill.overprint = true;
        let mut appearance = Appearance::default();
        appearance.items.clear();
        appearance.items.push(AppearanceItem::Fill(fill));
        let mut trap = Node::path(id, prepared.band.clone(), appearance);
        trap.name = Some(name);
        trap.edit_attrs(|a| a.data.push((TAG.into(), metadata)));
        d.insert(Some(layer_id), usize::MAX, trap)?;
        sel.set([id]);
        Ok((id, layer_id))
    })?;
    // Consume only after the atomic edit succeeds; a double click cannot insert twice.
    s.local_trap_preview = None;
    Ok(json!({"status":"incorporated","id":trap_id,"layerId":layer_id,"source":prepared.record.source,"receiver":prepared.record.receiver,
        "widthMm":prepared.record.width_mm,"overprint":true,"sourcePaint":prepared.paint,"experimental":true}))
}
