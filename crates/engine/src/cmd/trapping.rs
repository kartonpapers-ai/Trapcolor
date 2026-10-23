//! Trap Color's native adapter for the steppa-trap geometry crate.
//! The panel and automation use the same validated, transactional commands.

use serde_json::{Value, json};
use vectorcraft_color::{BlendMode, Color, Paint};
use vectorcraft_doc::{AppearanceItem, NodeKind};
use vectorcraft_plugins::objects;
use vectorcraft_trapping::{Modo, Opciones, PT_POR_MM, atrapar_con, leer_ajuste};

use super::*;

const APPLY: &str = "trapping.apply";
// The original plug-in had interpreter budgets. Bound the native selection too.
const MAX_INPUTS: usize = 80;
const MAX_POINTS: usize = 12_000;

fn number(p: &Value, key: &str, default: f64, min: f64, max: f64) -> Result<f64> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite() && (min..=max).contains(n))
            .ok_or_else(|| bad(APPLY, format!("{key}: introduce un valor entre {min} y {max}."))),
    }
}

fn choice<'a>(p: &'a Value, key: &str, default: &'a str) -> Result<&'a str> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v.as_str().ok_or_else(|| bad(APPLY, format!("{key} debe ser texto."))),
    }
}

fn options(p: &Value) -> Result<Opciones> {
    let modo = match choice(p, "direction", "auto")? {
        "auto" => Modo::Auto,
        "reverse" => Modo::Invertido,
        "center" => Modo::Centrado,
        _ => return Err(bad(APPLY, "direction debe ser auto, reverse o center.")),
    };
    let negro_sobreimprime = match choice(p, "black", "overprint")? {
        "overprint" => true,
        "trap" => false,
        _ => return Err(bad(APPLY, "black debe ser overprint o trap.")),
    };
    Ok(Opciones {
        ancho: number(p, "widthMm", 0.1, 0.01, 2.0)? * PT_POR_MM,
        modo,
        negro_sobreimprime,
        diferencia_minima: number(p, "difference", 10.0, 0.0, 100.0)? / 100.0,
    })
}

fn targets(s: &Session, p: &Value) -> Result<Vec<NodeId>> {
    let st = s.doc()?;
    let selected = ids_param(p, "ids").unwrap_or_else(|| st.selection.objects.clone());
    Ok(objects::filter_targets(&st.doc, &selected))
}

fn black_labels(s: &Session, ids: &[NodeId]) -> Result<Vec<String>> {
    let doc = &s.doc()?.doc;
    Ok(ids
        .iter()
        .filter_map(|id| doc.node(*id))
        .filter_map(|n| {
            if n.name.as_deref().and_then(leer_ajuste).is_some() {
                return None;
            }
            let encoded = serde_json::to_value(n.appearance.fill_paint().color()?).ok()?;
            let c = vectorcraft_trapping::Color::leer(&encoded)?;
            c.es_negro().then(|| vectorcraft_trapping::etiqueta(&c))
        })
        .collect())
}

/// Cheap read-only snapshot. Never calculates intersections during UI frames.
pub fn snapshot(s: &Session) -> Result<Value> {
    info(s, &json!({}))
}

fn info(s: &Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let ids = targets(s, p)?;
    let selected = ids_param(p, "ids").unwrap_or_else(|| st.selection.objects.clone());
    let blacks = black_labels(s, &ids)?;
    let mut pairs = Vec::new();
    let mut artwork = 0;
    for id in &ids {
        let Some(n) = st.doc.node(*id) else { continue };
        match n.name.as_deref().and_then(leer_ajuste) {
            Some(Ok(Some(a))) => pairs.push(json!({
                "id": id.0, "name": n.display_name(), "left": a.izq, "right": a.der,
                "relation": a.rel.to_string(), "widthMm": a.ancho_mm,
                "blackLeft": blacks.contains(&a.izq), "blackRight": blacks.contains(&a.der),
            })),
            Some(_) => {}
            None => artwork += 1,
        }
    }
    Ok(json!({"ids": selected.iter().map(|id| id.0).collect::<Vec<_>>(), "artwork": artwork, "pairs": pairs}))
}

fn query(s: &mut Session, p: &Value) -> Result<Value> {
    info(s, p)
}

/// Unsupported art must fail before an edit, rather than produce plausible but wrong traps.
fn validate(s: &Session, ids: &[NodeId]) -> Result<()> {
    if ids.len() > MAX_INPUTS {
        return Err(bad(APPLY, "Selecciona como máximo 80 trazados, incluidos los traps. Trabaja por partes."));
    }
    let doc = &s.doc()?.doc;
    let mut count = 0;
    let mut artwork = 0;
    for id in ids {
        let n = doc.node(*id).ok_or_else(|| bad(APPLY, "Un objeto ya no existe. Vuelve a seleccionarlo."))?;
        let control = n.name.as_deref().and_then(leer_ajuste).is_some();
        if let Some((path, _, _)) = objects::geometry(n) {
            count += path.anchor_count();
            for (_, _, a) in path.anchors() {
                if [a.p, a.h_in, a.h_out].iter().any(|v| !v.x.is_finite() || !v.y.is_finite() || v.x.abs() > 4e6 || v.y.abs() > 4e6) {
                    return Err(bad(APPLY, "El trazado contiene coordenadas fuera de rango."));
                }
            }
        }
        if control {
            continue;
        }
        artwork += 1;
        let fills: Vec<_> = n
            .appearance
            .items
            .iter()
            .filter_map(|i| match i {
                AppearanceItem::Fill(f) => Some(f),
                _ => None,
            })
            .collect();
        let solid = matches!(fills.as_slice(), [f] if matches!(&f.paint,
            Paint::Solid { color: Color::Cmyk { .. } | Color::Rgb { .. } | Color::Gray { .. }, swatch: None, tint } if *tint == 1.0)
            && f.visible && !f.overprint && f.opacity == 1.0 && f.blend == BlendMode::Normal && f.effects.is_empty());
        let strokes =
            n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Stroke(st) if st.width > 0.0 && !matches!(st.paint, Paint::None)));
        if !solid || strokes || n.opacity != 1.0 || n.blend != BlendMode::Normal || !n.appearance.effects.is_empty() || n.extras().mask.is_some() {
            return Err(bad(
                APPLY,
                "Esta versión necesita trazados con un relleno plano de proceso, sin contorno, efectos ni transparencia. Convierte las muestras a color de proceso.",
            ));
        }
        let mut parent = doc.parent_of(*id);
        while let Some(pid) = parent {
            let Some(p) = doc.node(pid) else { break };
            if p.opacity != 1.0
                || p.blend != BlendMode::Normal
                || p.extras().mask.is_some()
                || !p.appearance.effects.is_empty()
                || matches!(p.kind, NodeKind::Group { clip: true, .. } | NodeKind::Layer { clip: true, .. })
            {
                return Err(bad(APPLY, "Saca los trazados de grupos con recorte, efectos o transparencia antes de aplicar trapping."));
            }
            parent = doc.parent_of(pid);
        }
    }
    if count > MAX_POINTS {
        return Err(bad(APPLY, "La selección supera 12.000 nodos. Simplifica o trabaja por partes."));
    }
    if artwork < 2 {
        return Err(bad(APPLY, "Selecciona al menos dos trazados de colores distintos y sus traps, si ya existen."));
    }
    Ok(())
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let opc = options(p)?;
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let selected = ids_param(p, "ids").unwrap_or_else(|| st.selection.objects.clone());
    for id in &selected {
        let Some(node) = st.doc.node(*id) else {
            return Err(bad(APPLY, "Un objeto seleccionado ya no existe."));
        };
        let mut unsupported = false;
        node.walk(&mut |n| {
            if n.visible
                && !n.locked
                && !matches!(n.kind, NodeKind::Group { .. } | NodeKind::Layer { .. } | NodeKind::Path { .. } | NodeKind::Compound { .. })
            {
                unsupported = true;
            }
        });
        if unsupported {
            return Err(bad(APPLY, "La selección contiene textos, imágenes u objetos vivos. Usa solo trazados de relleno plano."));
        }
    }
    validate(s, &ids)?;
    let st = s.doc()?;
    let parent = st.insertion_parent();
    let blacks = black_labels(s, &ids)?;
    let bytes = objects::encode_input(&st.doc, &ids);
    let mut input: Value = serde_json::from_slice(&bytes).map_err(|e| bad(APPLY, e.to_string()))?;
    let reset =
        p.get("resetExisting").map(|v| v.as_bool().ok_or_else(|| bad(APPLY, "resetExisting debe ser booleano."))).transpose()?.unwrap_or(false);
    if reset {
        for o in input.get_mut("objects").and_then(Value::as_array_mut).into_iter().flatten() {
            if o.get("name").and_then(Value::as_str).and_then(leer_ajuste).is_some() {
                o["name"] = json!("trap");
            }
        }
    }
    // Pair changes are made to the calculation input, never as a separate document edit.
    if let Some(pair) = p.get("pair") {
        let id = pair.get("id").and_then(Value::as_u64).ok_or_else(|| bad(APPLY, "pair.id es obligatorio."))?;
        let o = input
            .get_mut("objects")
            .and_then(Value::as_array_mut)
            .and_then(|v| v.iter_mut().find(|o| o["id"] == id))
            .ok_or_else(|| bad(APPLY, "Selecciona el arte y el trap del par que quieres cambiar."))?;
        let mut a = o
            .get("name")
            .and_then(Value::as_str)
            .and_then(leer_ajuste)
            .and_then(|a| a.ok())
            .flatten()
            .ok_or_else(|| bad(APPLY, "El objeto seleccionado no es un trap con ajuste."))?;
        a.ancho_mm = number(pair, "widthMm", 0.1, 0.01, 2.0)?;
        match choice(pair, "mode", "forward")? {
            "forward" => a.rel = '>',
            "reverse" => {
                std::mem::swap(&mut a.izq, &mut a.der);
                a.rel = '>';
            }
            "center" => a.rel = '=',
            "none" => a.rel = '-',
            "overprint" if blacks.contains(&a.izq) || blacks.contains(&a.der) => {
                if blacks.contains(&a.der) {
                    std::mem::swap(&mut a.izq, &mut a.der);
                }
                a.rel = 'S';
            }
            _ => return Err(bad(APPLY, "Modo de par inválido; sobreimpresión solo admite negro K.")),
        }
        o["name"] = json!(a.nombre());
    }
    let value = atrapar_con(&input, &opc);
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(bad(APPLY, error));
    }
    let out_bytes = serde_json::to_vec(&value).map_err(|e| bad(APPLY, e.to_string()))?;
    let output = objects::decode(&out_bytes).map_err(|e| bad(APPLY, e.to_string()))?;
    let result = s.edit("Trapping", |d, sel| {
        let result = objects::apply_output(d, &ids, output, parent).map_err(|e| bad(APPLY, e.to_string()))?;
        sel.set(result.iter().copied());
        Ok(result)
    })?;
    let mut report = snapshot(s)?;
    report["ids"] = json!(result.iter().map(|id| id.0).collect::<Vec<_>>());
    Ok(report)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "trapping.info", "Trapping Info", [], None,
            "{ids?} → {ids, artwork, pairs: [{id, name, left, right, relation, widthMm}]}; no geometry calculation", has_doc, query),
        cmd!(
            "trapping.apply",
            "Apply Trapping",
            [],
            None,
            "{ids?, widthMm?: 0.01..2, direction?: auto|reverse|center, black?: overprint|trap, difference?: 0..100, resetExisting?: bool, pair?: {id, widthMm, mode: forward|reverse|center|none|overprint}} steppa-trap 0.4 process-color geometry as one undo step; overprint is simulated with combined CMYK, spot links are not supported",
            has_doc,
            apply
        ),
    ]
}
