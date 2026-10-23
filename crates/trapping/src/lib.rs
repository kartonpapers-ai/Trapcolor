//! Lógica del trapping, sin nada de WebAssembly: se puede probar con `cargo test`.
//!
//! Para cada par de objetos con relleno plano que se tocan (A abajo, B arriba) se busca la zona
//! de contacto visible y se crea un objeto "Trap" justo encima de B:
//! - el color más claro se expande hacia el más oscuro, `ancho` puntos (modo "auto"),
//!   o mitad y mitad sobre el borde (modo "centrado");
//! - el color del trap es la suma de las tintas de los dos (máximo por canal CMYK), que es lo
//!   que daría sobreimprimir el claro sobre el oscuro. Así el trap sale bien aunque quede calado,
//!   porque la API de plugins todavía no permite marcar sobreimpresión.
//!
//! Coordenadas en puntos, Y hacia abajo (como las da VectorCraft).

// Adapted from kartonpapers-ai/steppa-trap v0.4.0, src/trap.rs
// Source blob: 349ab24379f7e4b9577401555e5b17aa95553ae7. MIT OR Apache-2.0.
// The ABI wrapper is deliberately excluded: this crate is shared by native and web engines.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::outline::offset::OutlineOffset;
use i_overlay::mesh::style::{LineJoin, OutlineStyle};
use serde_json::{Value, json};

type Pt = [f64; 2];
type Contorno = Vec<Pt>;
type Forma = Vec<Contorno>;

/// Puntos por milímetro.
pub const PT_POR_MM: f64 = 72.0 / 25.4;
/// Tolerancia al convertir curvas en segmentos, en puntos (≈ 0,035 mm): lo más que se aparta
/// el polígono de la curva. Pocos puntos importan: VectorCraft limita el cálculo de un plugin.
const TOLERANCIA: f64 = 0.1;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Modo {
    /// El color claro se expande hacia el oscuro.
    Auto,
    /// Al revés: el oscuro se expande hacia el claro.
    Invertido,
    /// Mitad del ancho a cada lado del borde.
    Centrado,
}

/// Las opciones del diálogo, para los pares de colores que todavía no tienen trap.
#[derive(Clone, Copy, Debug)]
pub struct Opciones {
    /// Ancho del trap, en puntos.
    pub ancho: f64,
    pub modo: Modo,
    /// El negro (K solo) sobreimprime en vez de atraparse.
    pub negro_sobreimprime: bool,
    /// Diferencia mínima de tinta (0–1) para atrapar: con menos, los colores comparten tinta y
    /// no se ve blanco si el registro se corre.
    pub diferencia_minima: f64,
}

impl Opciones {
    /// Sin reglas: todo par de colores distintos se atrapa.
    pub fn simples(ancho: f64, modo: Modo) -> Self {
        Opciones { ancho, modo, negro_sobreimprime: false, diferencia_minima: 0.0 }
    }
}

/// Un color de relleno plano.
#[derive(Clone, Debug, PartialEq)]
pub enum Color {
    Cmyk([f64; 4]),
    Rgb([f64; 3]),
    /// Gris como tinta: 0 blanco, 1 negro.
    Gris(f64),
    Lab([f64; 3]),
}

impl Color {
    /// Read the same serialized process color used by the host adapter.
    pub fn leer(v: &Value) -> Option<Color> {
        let n = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        match v.get("model")?.as_str()? {
            "cmyk" => Some(Color::Cmyk([n("c"), n("m"), n("y"), n("k")])),
            "rgb" => Some(Color::Rgb([n("r"), n("g"), n("b")])),
            "gray" => Some(Color::Gris(n("k"))),
            "lab" => Some(Color::Lab([n("l"), n("a"), n("b")])),
            _ => None,
        }
    }

    fn json(&self) -> Value {
        match *self {
            Color::Cmyk([c, m, y, k]) => json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k}),
            Color::Rgb([r, g, b]) => json!({"model": "rgb", "r": r, "g": g, "b": b}),
            Color::Gris(k) => json!({"model": "gray", "k": k}),
            Color::Lab([l, a, b]) => json!({"model": "lab", "l": l, "a": a, "b": b}),
        }
    }

    /// Claridad aproximada, 0 (negro) a 1 (blanco). Alcanza para decidir qué color se expande.
    pub fn claridad(&self) -> f64 {
        let luma = |r: f64, g: f64, b: f64| 0.2126 * r + 0.7152 * g + 0.0722 * b;
        match *self {
            Color::Cmyk([c, m, y, k]) => luma((1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)),
            Color::Rgb([r, g, b]) => luma(r, g, b),
            Color::Gris(k) => 1.0 - k,
            Color::Lab([l, _, _]) => l / 100.0,
        }
    }

    /// Si es negro de una sola tinta (K ≥ 95 % y casi nada de C, M, Y).
    pub fn es_negro(&self) -> bool {
        // El host guarda color en f32: conserva los umbrales 95 % / 5 % al serializarlo.
        self.tintas().is_some_and(|[c, m, y, k]| k >= 0.95 - 1e-7 && c.max(m).max(y) <= 0.05 + 1e-7)
    }

    /// Las tintas CMYK del color. El RGB (por ejemplo, un SVG) se pasa a CMYK de forma simple,
    /// sin perfil: #00ffff es C 100, #ff00ff es M 100, #ffff00 es Y 100.
    fn tintas(&self) -> Option<[f64; 4]> {
        match *self {
            Color::Cmyk(c) => Some(c),
            Color::Gris(k) => Some([0.0, 0.0, 0.0, k]),
            Color::Rgb([r, g, b]) => {
                let k = 1.0 - r.max(g).max(b);
                if k >= 1.0 {
                    return Some([0.0, 0.0, 0.0, 1.0]);
                }
                let t = |x: f64| ((1.0 - x - k) / (1.0 - k)).clamp(0.0, 1.0);
                Some([t(r), t(g), t(b), k])
            }
            Color::Lab(_) => None,
        }
    }
}

/// El color del trap: las tintas de los dos sumadas (máximo por canal) cuando ambos son de
/// proceso; si no, el color claro (y entonces hay que sobreimprimirlo a mano).
pub fn color_trap(a: &Color, b: &Color) -> (Color, bool) {
    if let (Some(x), Some(y)) = (a.tintas(), b.tintas()) {
        return (Color::Cmyk([x[0].max(y[0]), x[1].max(y[1]), x[2].max(y[2]), x[3].max(y[3])]), true);
    }
    let claro = if a.claridad() >= b.claridad() { a } else { b };
    (claro.clone(), false)
}

/// La diferencia de tinta entre dos colores (0–1): en cuánto supera cada uno al otro en alguna
/// tinta, la menor de las dos. Rojo y naranja comparten M e Y: diferencia baja, no hace falta trap.
pub fn diferencia_tinta(a: &Color, b: &Color) -> Option<f64> {
    let (x, y) = (a.tintas()?, b.tintas()?);
    let mas = |p: &[f64; 4], q: &[f64; 4]| (0..4).map(|i| p[i] - q[i]).fold(0.0, f64::max);
    Some(mas(&x, &y).min(mas(&y, &x)))
}

/// Un objeto de la selección que pinta un relleno.
struct Objeto {
    forma: Vec<Forma>,
    caja: [f64; 4],
    /// Si tiene curvas (sus polígonos quedan hasta `TOLERANCIA` por dentro de la curva real).
    curvo: bool,
    /// El relleno plano visible (el de más arriba), si lo es.
    color: Option<Color>,
}

fn punto(v: Option<&Value>) -> Option<Pt> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

/// Los subtrazados del objeto como polígonos (las curvas, en segmentos).
fn poligonos(path: &Value) -> Vec<Contorno> {
    let mut out = Vec::new();
    for sub in path["subpaths"].as_array().into_iter().flatten() {
        let anclas: Vec<&Value> = sub["anchors"].as_array().into_iter().flatten().collect();
        if anclas.len() < 2 {
            continue;
        }
        let mut c: Contorno = Vec::new();
        // Al rellenar, un trazado abierto se cierra solo: siempre se recorre el último tramo.
        for i in 0..anclas.len() {
            let (a, b) = (anclas[i], anclas[(i + 1) % anclas.len()]);
            let (Some(p0), Some(p3)) = (punto(a.get("p")), punto(b.get("p"))) else { continue };
            let p1 = punto(a.get("out")).unwrap_or(p0);
            let p2 = punto(b.get("in")).unwrap_or(p3);
            c.push(p0);
            if p1 != p0 || p2 != p3 {
                // Bézier cúbica: tantos tramos como pida la tolerancia.
                // Cota de Wang: con n tramos el error es ≤ 3/4 · M / n², M = mayor segunda diferencia.
                let segunda = |a: Pt, b: Pt, c: Pt| (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1]);
                let m = segunda(p0, p1, p2).max(segunda(p1, p2, p3));
                let n = ((0.75 * m / TOLERANCIA).sqrt().ceil() as usize).clamp(1, 64);
                for k in 1..n {
                    let t = k as f64 / n as f64;
                    let u = 1.0 - t;
                    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    c.push([w0 * p0[0] + w1 * p1[0] + w2 * p2[0] + w3 * p3[0], w0 * p0[1] + w1 * p1[1] + w2 * p2[1] + w3 * p3[1]]);
                }
            }
        }
        if c.len() >= 3 {
            out.push(c);
        }
    }
    out
}

/// Caja `[x0, y0, x1, y1]` de las formas (vacía: x0 > x1).
fn caja_de(f: &[Forma]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in f.iter().flatten().flatten() {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    b
}

/// Si las cajas se tocan, con `margen` de holgura.
fn cerca(a: &[f64; 4], b: &[f64; 4], margen: f64) -> bool {
    a[0] <= b[2] + margen && b[0] <= a[2] + margen && a[1] <= b[3] + margen && b[1] <= a[3] + margen
}

fn leer_objeto(o: &Value) -> Option<Objeto> {
    o.get("id")?.as_u64()?;
    let fills: Vec<&Value> = o["fills"].as_array().into_iter().flatten().filter(|f| !f.is_null()).collect();
    let arriba = fills.last()?; // sin relleno no tapa nada ni lleva trap
    let color = (arriba["type"] == "solid").then(|| Color::leer(&arriba["color"])).flatten();
    let regla = if o["fillRule"].as_str() == Some("evenodd") { FillRule::EvenOdd } else { FillRule::NonZero };
    let forma = poligonos(&o["path"]).simplify_shape(regla);
    let caja = caja_de(&forma);
    let curvo = o["path"]["subpaths"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["anchors"].as_array().into_iter().flatten())
        .any(|a| a.get("in").is_some() || a.get("out").is_some());
    (!forma.is_empty()).then_some(Objeto { forma, caja, curvo, color })
}

/// Ensancha (`d` > 0) o achica (`d` < 0) las formas.
fn ensanchar(f: &[Forma], d: f64) -> Vec<Forma> {
    f.to_vec().outline(&OutlineStyle::new(d).line_join(LineJoin::Miter(0.1 * std::f64::consts::PI)))
}

fn op(a: &[Forma], b: &[Forma], regla: OverlayRule) -> Vec<Forma> {
    if a.is_empty() {
        return Vec::new();
    }
    if b.is_empty() {
        return match regla {
            OverlayRule::Intersect => Vec::new(),
            _ => a.to_vec(),
        };
    }
    a.to_vec().overlay(&b.to_vec(), regla, FillRule::NonZero)
}

/// Área total (con signo de los contornos: los huecos restan).
fn area(f: &[Forma]) -> f64 {
    f.iter()
        .flatten()
        .map(|c| {
            let n = c.len();
            (0..n).map(|i| c[i][0] * c[(i + 1) % n][1] - c[(i + 1) % n][0] * c[i][1]).sum::<f64>() / 2.0
        })
        .sum::<f64>()
        .abs()
}

/// Quién se expande en un par de colores.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Expande {
    /// El de arriba (B) crece sobre el de abajo.
    Arriba,
    /// El de abajo (A) crece por debajo del de arriba.
    Abajo,
    /// Mitad del ancho a cada lado del borde.
    Centro,
    /// El de arriba sobreimprime: el trap cubre todo lo que tapa de A (con las tintas de los dos).
    Sobre,
}

/// La zona de trap entre `a` (abajo, ya sin lo que la tapa) y `b` (arriba).
fn zona(a: &[Forma], b: &[Forma], expande: Expande, ancho: f64) -> Vec<Forma> {
    // Lo que se ve de A alrededor de B.
    let a_libre = op(a, b, OverlayRule::Difference);
    match expande {
        Expande::Arriba => op(&ensanchar(b, ancho), &a_libre, OverlayRule::Intersect),
        Expande::Abajo => op(&ensanchar(&a_libre, ancho), b, OverlayRule::Intersect),
        Expande::Centro => {
            let fuera = op(&ensanchar(b, ancho / 2.0), &a_libre, OverlayRule::Intersect);
            let dentro = op(&ensanchar(&a_libre, ancho / 2.0), b, OverlayRule::Intersect);
            op(&fuera, &dentro, OverlayRule::Union)
        }
        Expande::Sobre => op(a, b, OverlayRule::Intersect),
    }
}

fn trazado(formas: &[Forma]) -> Value {
    let subpaths: Vec<Value> =
        formas.iter().flatten().map(|c| json!({"closed": true, "anchors": c.iter().map(|p| json!({"p": p})).collect::<Vec<_>>()})).collect();
    json!({"subpaths": subpaths})
}

/// Nombre corto de un color por sus tintas: "Y", "C", "CM", "M50Y", "K"… (100 % no se escribe).
pub fn etiqueta(c: &Color) -> String {
    match c.tintas() {
        Some(t) => {
            let mut s = String::new();
            for (letra, v) in ['C', 'M', 'Y', 'K'].into_iter().zip(t) {
                let pct = (v * 100.0).round() as i64;
                if pct > 0 {
                    s.push(letra);
                    if pct < 100 {
                        s.push_str(&pct.to_string());
                    }
                }
            }
            if s.is_empty() { "W".into() } else { s }
        }
        None => match c {
            Color::Lab([l, a, b]) => format!("L{}A{}B{}", l.round(), a.round(), b.round()),
            _ => "?".into(),
        },
    }
}

/// Lo que el usuario escribió en el nombre de un trap: `trap Y>C 0.5` (Y se expande sobre C),
/// `trap Y=C 0.5` (centrado), `trap K/Y 0.1` (el negro sobreimprime al amarillo), `trap Y-C no`
/// (sin trap).
#[derive(Clone, Debug, PartialEq)]
pub struct Ajuste {
    /// El color que se expande (con `>`), o el primero del par.
    pub izq: String,
    pub der: String,
    /// `>` izq crece sobre der, `=` centrado, `S` izq (el negro) sobreimprime a der (se escribe
    /// `/`), `-` sin trap.
    pub rel: char,
    /// Ancho en mm (no hay con `-`).
    pub ancho_mm: f64,
}

impl Ajuste {
    pub fn nombre(&self) -> String {
        if self.rel == '-' {
            format!("trap {}-{} no", self.izq, self.der)
        } else {
            let rel = if self.rel == 'S' { '/' } else { self.rel };
            format!("trap {}{}{} {:.2}", self.izq, rel, self.der, self.ancho_mm)
        }
    }

    fn es_del_par(&self, x: &str, y: &str) -> bool {
        (self.izq == x && self.der == y) || (self.izq == y && self.der == x)
    }
}

/// Lee el nombre de un objeto: `None` si no es un trap; `Some(Ok(None))` si es un trap sin
/// ajuste (como los de la versión 0.2, `trap` a secas); `Some(Err(..))` si no se entiende.
pub fn leer_ajuste(nombre: &str) -> Option<Result<Option<Ajuste>, String>> {
    let n = nombre.trim();
    if !n.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("trap")) {
        return None;
    }
    let resto = n[4..].trim();
    if resto.is_empty() || resto.starts_with('(') {
        return Some(Ok(None));
    }
    let error = || Err(format!("No entiendo el trap \"{n}\". Ejemplos: trap Y>C 0.5 · trap C>Y 0.1 · trap Y=C 0.2 · trap K/Y 0.1 · trap Y-C no"));
    // Ancho (el último número) o "no"; el resto es la relación, con o sin espacios.
    let mut partes: Vec<&str> = resto.split_whitespace().collect();
    let ultimo = partes.pop().unwrap_or_default();
    let (sin, ancho_mm) = if ultimo.eq_ignore_ascii_case("no") {
        (true, 0.0)
    } else {
        match ultimo.replace(',', ".").trim_end_matches("mm").parse::<f64>() {
            Ok(v) if (0.001..=5.0).contains(&v) => (false, v),
            _ => return Some(error()),
        }
    };
    let rel: String = partes.concat().to_ascii_uppercase();
    let Some(i) = rel.find(['>', '<', '=', '-', '/']) else { return Some(error()) };
    let (a, signo, b) = (&rel[..i], rel.as_bytes()[i] as char, &rel[i + 1..]);
    if a.is_empty() || b.is_empty() {
        return Some(error());
    }
    let (izq, der) = if signo == '<' { (b, a) } else { (a, b) };
    let rel = match (sin, signo) {
        (true, _) => '-',
        (false, '<') => '>',
        (false, '/') => 'S',
        (false, '-') => return Some(error()),
        (false, c) => c,
    };
    Some(Ok(Some(Ajuste { izq: izq.into(), der: der.into(), rel, ancho_mm })))
}

/// Un par de colores que se tocan, con su trap.
struct Par {
    ajuste: Ajuste,
    color: Color,
    zonas: Vec<Forma>,
}

/// Ejecuta el filtro: `entrada` es `{"objects": [...]}` de VectorCraft. Devuelve la salida
/// (`{"objects": [...]}` o `{"error": "..."}`).
///
/// Los objetos que se llaman `trap …` son los traps de una pasada anterior: su nombre dice cómo
/// atrapar ese par de colores (ancho, dirección, centrado o sin trap). Se recalculan en su lugar;
/// los pares nuevos usan `ancho` (pt) y `modo`.
pub fn atrapar(entrada: &Value, ancho: f64, modo: Modo) -> Value {
    atrapar_con(entrada, &Opciones::simples(ancho, modo))
}

/// [`atrapar`] con las reglas del diálogo: negro que sobreimprime y diferencia mínima de tinta.
/// Un par que queda sin trap por una regla igual aparece (`trap X-Y no`, sin relleno), para
/// poder forzarlo renombrándolo.
pub fn atrapar_con(entrada: &Value, opc: &Opciones) -> Value {
    let (ancho, modo) = (opc.ancho, opc.modo);
    let originales: Vec<&Value> = entrada["objects"].as_array().into_iter().flatten().collect();
    // Separar los traps anteriores (controles) del arte.
    let mut controles: Vec<(Value, Option<Ajuste>)> = Vec::new();
    let mut arte: Vec<&Value> = Vec::new();
    for o in &originales {
        match o["name"].as_str().and_then(leer_ajuste) {
            Some(Ok(a)) => controles.push((o["id"].clone(), a)),
            Some(Err(e)) => return json!({"error": e}),
            None => arte.push(o),
        }
    }
    let objetos: Vec<Objeto> = arte.iter().filter_map(|o| leer_objeto(o)).collect();
    if objetos.iter().filter(|o| o.color.is_some()).count() < 2 {
        return json!({"error": "Selecciona al menos dos objetos con relleno plano que se toquen."});
    }
    let ancho_mm_def = ancho / PT_POR_MM;
    let mut pares: Vec<Par> = Vec::new();
    for j in 1..objetos.len() {
        let b = &objetos[j];
        let Some(cb) = &b.color else { continue };
        for i in 0..j {
            let a = &objetos[i];
            let Some(ca) = &a.color else { continue };
            let (la, lb) = (etiqueta(ca), etiqueta(cb));
            if la == lb {
                continue;
            }
            // Ajuste del par: el escrito en un trap anterior, el ya decidido, o el de por defecto.
            let ajuste = pares
                .iter()
                .map(|p| &p.ajuste)
                .chain(controles.iter().filter_map(|c| c.1.as_ref()))
                .find(|aj| aj.es_del_par(&la, &lb))
                .cloned()
                .unwrap_or_else(|| {
                    let (claro, oscuro) = if cb.claridad() >= ca.claridad() { (&lb, &la) } else { (&la, &lb) };
                    let (izq, der) = if modo == Modo::Invertido { (oscuro, claro) } else { (claro, oscuro) };
                    let rel = if modo == Modo::Centrado { '=' } else { '>' };
                    let mut aj = Ajuste { izq: izq.clone(), der: der.clone(), rel, ancho_mm: ancho_mm_def };
                    if diferencia_tinta(ca, cb).is_some_and(|d| d < opc.diferencia_minima) {
                        aj.rel = '-';
                    } else if opc.negro_sobreimprime && (ca.es_negro() || cb.es_negro()) {
                        let (negro, otro) = if cb.es_negro() { (&lb, &la) } else { (&la, &lb) };
                        aj = Ajuste { izq: negro.clone(), der: otro.clone(), rel: 'S', ancho_mm: ancho_mm_def };
                    }
                    aj
                });
            // "Sin trap" escrito a mano no tiene ancho: se marca el par con el del diálogo.
            let w0 = if ajuste.ancho_mm > 0.0 { ajuste.ancho_mm * PT_POR_MM } else { ancho };
            if !cerca(&a.caja, &b.caja, w0) {
                continue;
            }
            let (color, _) = color_trap(ca, cb);
            let idx = match pares.iter().position(|p| p.ajuste.es_del_par(&la, &lb)) {
                Some(k) => k,
                None => {
                    pares.push(Par { ajuste: ajuste.clone(), color, zonas: Vec::new() });
                    pares.len() - 1
                }
            };
            // Lo que se ve de A: sin los objetos que tiene encima, por debajo de B.
            let mut a_vis = a.forma.clone();
            for occluder in objetos.iter().take(j).skip(i + 1) {
                if cerca(&a.caja, &occluder.caja, 0.0) {
                    a_vis = op(&a_vis, &occluder.forma, OverlayRule::Difference);
                }
            }
            let expande = match ajuste.rel {
                '=' => Expande::Centro,
                // El negro arriba sobreimprime; abajo, el otro color crece sobre él.
                'S' if ajuste.izq == lb => Expande::Sobre,
                'S' => Expande::Arriba,
                // Sin trap: la zona solo marca dónde está el par (el objeto va sin relleno).
                '-' if cb.claridad() >= ca.claridad() => Expande::Arriba,
                '-' => Expande::Abajo,
                _ if ajuste.izq == lb => Expande::Arriba,
                _ => Expande::Abajo,
            };
            // Con curvas, el polígono queda un poco por dentro: se compensa con media tolerancia.
            let w = if a.curvo || b.curvo { w0 + TOLERANCIA / 2.0 } else { w0 };
            let mut z = zona(&a_vis, &b.forma, expande, w);
            // El trap va encima de toda la selección: se le quita lo que tapa a B.
            let cz = caja_de(&z);
            for occluder in objetos.iter().skip(j + 1) {
                if cerca(&cz, &occluder.caja, 0.0) {
                    z = op(&z, &occluder.forma, OverlayRule::Difference);
                }
            }
            if area(&z) >= (w0 * w0 * 0.01).max(1e-4) {
                pares[idx].zonas.extend(z);
            }
        }
    }
    // Un par sin contacto real no lleva trap.
    pares.retain(|p| !p.zonas.is_empty());
    if pares.is_empty() {
        return json!({"error": "No hay colores distintos que se toquen en la selección."});
    }
    // El arte queda como está. Cada par actualiza su trap anterior (así sigue en su capa) o crea
    // uno nuevo encima de todo; los traps anteriores que ya no corresponden se borran.
    let mut salida: Vec<Value> = arte.iter().map(|o| json!({"id": o["id"]})).collect();
    let mut usados = vec![false; controles.len()];
    let mut nuevos = Vec::new();
    for p in &pares {
        let z = p.zonas.simplify_shape(FillRule::NonZero);
        let tipo = if z.iter().map(Vec::len).sum::<usize>() > 1 { "compound" } else { "path" };
        // Sin trap: el objeto queda sin relleno, para ver el par y poder forzarlo.
        let fills = if p.ajuste.rel == '-' { json!([]) } else { json!([{"type": "solid", "color": p.color.json()}]) };
        let mut obj = json!({"type": tipo, "name": p.ajuste.nombre(), "path": trazado(&z), "fillRule": "nonzero", "fills": fills});
        // Su trap anterior; si no tiene, uno viejo sin ajuste (`trap` a secas, de la versión 0.2).
        let libre = |k: usize, c: &(Value, Option<Ajuste>)| !usados[k] && c.1.as_ref().is_some_and(|a| a.es_del_par(&p.ajuste.izq, &p.ajuste.der));
        let previo = controles
            .iter()
            .enumerate()
            .position(|(k, c)| libre(k, c))
            .or_else(|| controles.iter().enumerate().position(|(k, c)| !usados[k] && c.1.is_none()));
        match previo {
            Some(k) => {
                usados[k] = true;
                obj["id"] = controles[k].0.clone();
                salida.push(obj);
            }
            None => nuevos.push(obj),
        }
    }
    salida.extend(nuevos);
    json!({"objects": salida})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuadrado(id: u64, x: f64, y: f64, lado: f64, color: Value) -> Value {
        let a = |px: f64, py: f64| json!({"p": [px, py]});
        json!({"id": id, "type": "path",
               "path": {"subpaths": [{"closed": true, "anchors": [a(x, y), a(x + lado, y), a(x + lado, y + lado), a(x, y + lado)]}]},
               "fillRule": "nonzero", "fills": [{"type": "solid", "color": color}]})
    }
    fn cmyk(c: f64, m: f64, y: f64, k: f64) -> Value {
        json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k})
    }
    fn traps(salida: &Value) -> Vec<Value> {
        salida["objects"].as_array().unwrap().iter().filter(|o| o.get("id").is_none()).cloned().collect()
    }
    fn caja(t: &Value) -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for s in t["path"]["subpaths"].as_array().unwrap() {
            for a in s["anchors"].as_array().unwrap() {
                let (x, y) = (a["p"][0].as_f64().unwrap(), a["p"][1].as_f64().unwrap());
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
        b
    }

    #[test]
    fn amarillo_sobre_negro_se_expande_hacia_afuera() {
        // Fondo negro 100×100 y un cuadrado amarillo 40×40 encima, en el medio.
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 1.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0))]});
        let s = atrapar(&e, 1.0, Modo::Auto);
        let t = traps(&s);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0]["fills"][0]["color"], cmyk(0.0, 0.0, 1.0, 1.0));
        assert_eq!(t[0]["name"], "trap Y>K 0.35");
        // El amarillo crece 1 pt hacia afuera: un anillo de 29..71.
        let c = caja(&t[0]);
        for (v, esperado) in c.iter().zip([29.0, 29.0, 71.0, 71.0]) {
            assert!((v - esperado).abs() < 1e-6, "{c:?}");
        }
        assert!(
            (area(&zona(&[cuadrado_forma(0.0, 100.0)], &[cuadrado_forma(30.0, 40.0)], Expande::Arriba, 1.0)) - (42.0 * 42.0 - 1600.0)).abs() < 1e-3
        );
        // El trap va justo encima del amarillo.
        let ids: Vec<Value> = s["objects"].as_array().unwrap().iter().map(|o| o["id"].clone()).collect();
        assert_eq!(ids, vec![json!(1), json!(2), Value::Null]);
    }

    fn cuadrado_forma(x: f64, lado: f64) -> Forma {
        vec![vec![[x, x], [x + lado, x], [x + lado, x + lado], [x, x + lado]]]
    }

    #[test]
    fn oscuro_sobre_claro_el_claro_entra_debajo() {
        // Fondo amarillo y un cuadrado negro encima: el amarillo se mete 1 pt dentro del negro.
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 1.0, 0.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 0.0, 1.0))]});
        let t = traps(&atrapar(&e, 1.0, Modo::Auto));
        assert_eq!(t.len(), 1);
        let c = caja(&t[0]);
        for (v, esperado) in c.iter().zip([30.0, 30.0, 70.0, 70.0]) {
            assert!((v - esperado).abs() < 1e-6, "{c:?}");
        }
    }

    #[test]
    fn centrado_reparte_el_ancho() {
        let a = [cuadrado_forma(0.0, 100.0)];
        let b = [cuadrado_forma(30.0, 40.0)];
        let z = zona(&a, &b, Expande::Centro, 2.0);
        // Anillo de 29 a 71 menos el de 31 a 69.
        assert!((area(&z) - (42.0 * 42.0 - 38.0 * 38.0)).abs() < 1e-3);
    }

    #[test]
    fn sin_contacto_no_hay_trap() {
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 10.0, cmyk(0.0, 0.0, 0.0, 1.0)),
                                   cuadrado(2, 50.0, 50.0, 10.0, cmyk(0.0, 0.0, 1.0, 0.0))]});
        assert!(atrapar(&e, 1.0, Modo::Auto).get("error").is_some());
    }

    #[test]
    fn mismo_color_o_uno_solo_no_atrapa() {
        let k = cmyk(0.0, 0.0, 0.0, 1.0);
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, k.clone()), cuadrado(2, 30.0, 30.0, 40.0, k.clone())]});
        assert!(atrapar(&e, 1.0, Modo::Auto).get("error").is_some());
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, k)]});
        assert!(atrapar(&e, 1.0, Modo::Auto).get("error").is_some());
    }

    #[test]
    fn objeto_intermedio_tapa_el_contacto() {
        // Negro abajo, blanco (cian 0) que tapa todo, amarillo arriba: el amarillo no toca el negro.
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 1.0)),
                                   cuadrado(2, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 0.0)),
                                   cuadrado(3, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0))]});
        let t = traps(&atrapar(&e, 1.0, Modo::Auto));
        // Solo el trap blanco–amarillo (el blanco es más claro: entra en el amarillo).
        assert_eq!(t.len(), 1);
        assert_eq!(t[0]["fills"][0]["color"], cmyk(0.0, 0.0, 1.0, 0.0));
    }

    #[test]
    fn rgb_se_pasa_a_tintas() {
        let rgb = |r: f64, g: f64, b: f64| json!({"model": "rgb", "r": r, "g": g, "b": b});
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, rgb(0.0, 1.0, 1.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, rgb(1.0, 1.0, 0.0))]});
        let t = traps(&atrapar(&e, 1.0, Modo::Auto));
        assert_eq!(t[0]["name"], "trap Y>C 0.35");
        assert_eq!(t[0]["fills"][0]["color"], cmyk(1.0, 0.0, 1.0, 0.0));
        let lab = |l: f64| json!({"model": "lab", "l": l, "a": 0.0, "b": 0.0});
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, lab(10.0)), cuadrado(2, 30.0, 30.0, 40.0, lab(90.0))]});
        let t = traps(&atrapar(&e, 1.0, Modo::Auto));
        assert_eq!(t[0]["fills"][0]["color"]["l"], 90.0);
    }

    #[test]
    fn tres_colores_traps_juntos_arriba() {
        // Como el SVG de prueba: cian abajo, magenta encima, amarillo arriba de los dos.
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(1.0, 0.0, 0.0, 0.0)),
                                   cuadrado(2, 60.0, 60.0, 80.0, cmyk(0.0, 1.0, 0.0, 0.0)),
                                   cuadrado(3, 40.0, 90.0, 60.0, cmyk(0.0, 0.0, 1.0, 0.0))]});
        let s = atrapar(&e, 1.0, Modo::Auto);
        let ids: Vec<Value> = s["objects"].as_array().unwrap().iter().map(|o| o["id"].clone()).collect();
        // Los tres objetos primero, después solo traps: uno por par de colores.
        assert_eq!(&ids[..3], &[json!(1), json!(2), json!(3)]);
        let t = traps(&s);
        let colores: Vec<&Value> = t.iter().map(|o| &o["fills"][0]["color"]).collect();
        let nombres: Vec<&str> = t.iter().map(|o| o["name"].as_str().unwrap()).collect();
        assert_eq!(nombres, vec!["trap C>M 0.35", "trap Y>C 0.35", "trap Y>M 0.35"]);
        assert_eq!(colores, vec![&cmyk(1.0, 1.0, 0.0, 0.0), &cmyk(1.0, 0.0, 1.0, 0.0), &cmyk(0.0, 1.0, 1.0, 0.0)]);
    }

    #[test]
    fn el_trap_no_asoma_sobre_lo_que_tapa() {
        // Negro abajo, amarillo encima y un cian que tapa medio amarillo: el trap Y→K no pasa por el cian.
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 1.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0)),
                                   cuadrado(3, 50.0, -10.0, 120.0, cmyk(1.0, 0.0, 0.0, 0.0))]});
        let t = traps(&atrapar(&e, 1.0, Modo::Auto));
        let yk = t.iter().find(|o| o["fills"][0]["color"] == cmyk(0.0, 0.0, 1.0, 1.0)).unwrap();
        assert!(caja(yk)[2] <= 50.0 + 1e-6, "{:?}", caja(yk));
    }

    #[test]
    fn curvas_se_convierten_en_segmentos() {
        // Un círculo de radio 10 con cuatro Bézier: el área debe acercarse a π·r².
        let k = 0.5523 * 10.0;
        let path = json!({"subpaths": [{"closed": true, "anchors": [
            {"p": [10.0, 0.0], "in": [10.0, -k], "out": [10.0, k]},
            {"p": [0.0, 10.0], "in": [k, 10.0], "out": [-k, 10.0]},
            {"p": [-10.0, 0.0], "in": [-10.0, k], "out": [-10.0, -k]},
            {"p": [0.0, -10.0], "in": [-k, -10.0], "out": [k, -10.0]}]}]});
        let f = poligonos(&path).simplify_shape(FillRule::NonZero);
        assert!((area(&f) - std::f64::consts::PI * 100.0).abs() < std::f64::consts::PI * 2.0, "{}", area(&f));
    }

    fn con_nombre(mut o: Value, nombre: &str) -> Value {
        o["name"] = json!(nombre);
        o
    }

    /// Ancho del anillo de un trap alrededor del cuadrado 30..70 (por la caja).
    fn anillo(t: &Value) -> f64 {
        let c = caja(t);
        30.0 - c[0]
    }

    #[test]
    fn leer_nombres_de_trap() {
        let aj = |izq: &str, der: &str, rel, ancho_mm| Some(Ok(Some(Ajuste { izq: izq.into(), der: der.into(), rel, ancho_mm })));
        assert_eq!(leer_ajuste("trap Y>C 0.5"), aj("Y", "C", '>', 0.5));
        assert_eq!(leer_ajuste("Trap  y > c  0,5mm"), aj("Y", "C", '>', 0.5));
        assert_eq!(leer_ajuste("trap Y<C 0.1"), aj("C", "Y", '>', 0.1));
        assert_eq!(leer_ajuste("trap Y=C 0.2"), aj("Y", "C", '=', 0.2));
        assert_eq!(leer_ajuste("trap Y-C no"), aj("Y", "C", '-', 0.0));
        assert_eq!(leer_ajuste("trap M50Y>K 0.25"), aj("M50Y", "K", '>', 0.25));
        assert_eq!(leer_ajuste("trap"), Some(Ok(None)));
        assert_eq!(leer_ajuste("trap (sobreimprimir)"), Some(Ok(None)));
        assert_eq!(leer_ajuste("Fondo"), None);
        assert!(matches!(leer_ajuste("trap Y?C 0.5"), Some(Err(_))));
        assert!(matches!(leer_ajuste("trap Y>C mucho"), Some(Err(_))));
        assert_eq!(Ajuste { izq: "Y".into(), der: "C".into(), rel: '>', ancho_mm: 0.5 }.nombre(), "trap Y>C 0.50");
        assert_eq!(etiqueta(&Color::Cmyk([0.0, 0.5, 1.0, 0.0])), "M50Y");
    }

    #[test]
    fn cambiar_ancho_invertir_centrar_y_quitar() {
        let arte = || vec![cuadrado(1, 0.0, 0.0, 100.0, cmyk(1.0, 0.0, 0.0, 0.0)), cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0))];
        let mm = PT_POR_MM;
        // Primera pasada: el amarillo (más claro) crece sobre el cian, 0,1 mm.
        let s = atrapar(&json!({"objects": arte()}), 0.1 * mm, Modo::Auto);
        let t = traps(&s);
        assert_eq!(t[0]["name"], "trap Y>C 0.10");
        // El usuario lo renombra (el trap ya existe como objeto 9): 0,5 mm.
        let pasada = |nombre: &str| {
            let mut objs = arte();
            objs.push(con_nombre(cuadrado(9, 0.0, 0.0, 1.0, cmyk(1.0, 0.0, 1.0, 0.0)), nombre));
            atrapar(&json!({"objects": objs}), 0.1 * mm, Modo::Auto)
        };
        let s = pasada("trap Y>C 0.5");
        let o = s["objects"].as_array().unwrap();
        assert_eq!(o.len(), 3, "se actualiza el mismo trap, no se crea otro");
        assert_eq!(o[2]["id"], 9);
        assert_eq!(o[2]["name"], "trap Y>C 0.50");
        assert!((anillo(&o[2]) - 0.5 * mm).abs() < 1e-6);
        // Invertido (forzado): el cian entra en el amarillo, el trap queda dentro del cuadrado.
        let s = pasada("trap C>Y 0.5");
        let o = &s["objects"][2];
        assert_eq!(o["name"], "trap C>Y 0.50");
        assert!((anillo(o) - 0.0).abs() < 1e-6, "{:?}", caja(o));
        // Centrado: mitad afuera.
        let s = pasada("trap Y=C 0.4");
        assert!((anillo(&s["objects"][2]) - 0.2 * mm).abs() < 1e-6);
        // Sin trap: el objeto queda, sin relleno, con su nombre.
        let s = pasada("trap Y-C no");
        let o = &s["objects"][2];
        assert_eq!((&o["id"], &o["name"], &o["fills"]), (&json!(9), &json!("trap Y-C no"), &json!([])));
        // Un trap viejo de la versión 0.2 se recalcula con los valores del diálogo.
        let s = pasada("trap");
        assert_eq!(s["objects"][2]["name"], "trap Y>C 0.10");
        assert_eq!(s["objects"][2]["id"], 9);
    }

    #[test]
    fn tres_colores_anchos_distintos_por_par() {
        // Ejemplo del pedido: amarillo a cian 0,5 y magenta a cian 0,1; el par Y-M sin tocar.
        let mm = PT_POR_MM;
        let arte = vec![
            cuadrado(1, 0.0, 0.0, 100.0, cmyk(1.0, 0.0, 0.0, 0.0)),
            cuadrado(2, 60.0, 60.0, 80.0, cmyk(0.0, 1.0, 0.0, 0.0)),
            cuadrado(3, 40.0, 90.0, 60.0, cmyk(0.0, 0.0, 1.0, 0.0)),
        ];
        let mut objs = arte.clone();
        objs.push(con_nombre(cuadrado(10, 0.0, 0.0, 1.0, cmyk(1.0, 1.0, 0.0, 0.0)), "trap C>M 0.1"));
        objs.push(con_nombre(cuadrado(11, 0.0, 0.0, 1.0, cmyk(1.0, 0.0, 1.0, 0.0)), "trap Y>C 0.5"));
        let s = atrapar(&json!({"objects": objs}), 0.2 * mm, Modo::Auto);
        let o = s["objects"].as_array().unwrap();
        let nombres: Vec<(Value, &str)> = o[3..].iter().map(|x| (x["id"].clone(), x["name"].as_str().unwrap())).collect();
        assert_eq!(nombres, vec![(json!(10), "trap C>M 0.10"), (json!(11), "trap Y>C 0.50"), (Value::Null, "trap Y>M 0.20")]);
    }

    #[test]
    fn trap_viejo_que_ya_no_corresponde_se_borra() {
        let mut objs = vec![cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 1.0)), cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0))];
        objs.push(con_nombre(cuadrado(9, 0.0, 0.0, 1.0, cmyk(1.0, 0.0, 0.0, 0.0)), "trap M>C 0.3"));
        let s = atrapar(&json!({"objects": objs}), 1.0, Modo::Auto);
        let ids: Vec<Value> = s["objects"].as_array().unwrap().iter().map(|o| o["id"].clone()).collect();
        assert_eq!(ids, vec![json!(1), json!(2), Value::Null]);
    }

    #[test]
    fn nombre_que_no_se_entiende_avisa() {
        let objs = vec![
            cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 1.0)),
            cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0)),
            con_nombre(cuadrado(9, 0.0, 0.0, 1.0, cmyk(1.0, 0.0, 0.0, 0.0)), "trap Y>K muchísimo"),
        ];
        let s = atrapar(&json!({"objects": objs}), 1.0, Modo::Auto);
        assert!(s["error"].as_str().unwrap().contains("trap Y>C 0.5"));
    }

    fn reglas(ancho_mm: f64) -> Opciones {
        Opciones { ancho: ancho_mm * PT_POR_MM, modo: Modo::Auto, negro_sobreimprime: true, diferencia_minima: 0.10 }
    }

    #[test]
    fn colores_que_comparten_tinta_no_se_atrapan_pero_se_pueden_forzar() {
        // Rojo (M100 Y100) y naranja (M50 Y100): comparten amarillo y magenta.
        let rojo = cmyk(0.0, 1.0, 1.0, 0.0);
        let naranja = cmyk(0.0, 0.5, 1.0, 0.0);
        assert_eq!(diferencia_tinta(&Color::leer(&rojo).unwrap(), &Color::leer(&naranja).unwrap()), Some(0.0));
        let arte = || vec![cuadrado(1, 0.0, 0.0, 100.0, rojo.clone()), cuadrado(2, 30.0, 30.0, 40.0, naranja.clone())];
        let s = atrapar_con(&json!({"objects": arte()}), &reglas(0.1));
        let t = traps(&s);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0]["name"], "trap M50Y-MY no");
        assert_eq!(t[0]["fills"], json!([]), "queda a la vista pero sin relleno");
        // Forzado por el usuario.
        let mut objs = arte();
        objs.push(con_nombre(cuadrado(9, 0.0, 0.0, 1.0, cmyk(0.0, 0.0, 0.0, 0.0)), "trap M50Y>MY 0.2"));
        let s = atrapar_con(&json!({"objects": objs}), &reglas(0.1));
        let o = &s["objects"][2];
        assert_eq!((&o["id"], &o["name"]), (&json!(9), &json!("trap M50Y>MY 0.20")));
        assert_eq!(o["fills"][0]["color"], cmyk(0.0, 1.0, 1.0, 0.0));
        assert!((anillo(o) - 0.2 * PT_POR_MM).abs() < 1e-6);
    }

    #[test]
    fn negro_encima_sobreimprime() {
        // Amarillo de fondo y un cuadrado negro encima: el trap cubre todo el negro, con Y + K.
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 1.0, 0.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 0.0, 1.0))]});
        let t = traps(&atrapar_con(&e, &reglas(0.1)));
        assert_eq!(t[0]["name"], "trap K/Y 0.10");
        assert_eq!(t[0]["fills"][0]["color"], cmyk(0.0, 0.0, 1.0, 1.0));
        assert_eq!(caja(&t[0]), [30.0, 30.0, 70.0, 70.0]);
        // Con la regla apagada, se atrapa normal (el amarillo entra bajo el negro).
        let t = traps(&atrapar_con(&e, &Opciones { negro_sobreimprime: false, ..reglas(0.1) }));
        assert_eq!(t[0]["name"], "trap Y>K 0.10");
    }

    #[test]
    fn negro_debajo_el_otro_color_crece_encima() {
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(0.0, 0.0, 0.0, 1.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0))]});
        let t = traps(&atrapar_con(&e, &reglas(0.5)));
        assert_eq!(t[0]["name"], "trap K/Y 0.50");
        assert!((anillo(&t[0]) - 0.5 * PT_POR_MM).abs() < 1e-6);
    }

    #[test]
    fn direccion_invertida_en_el_dialogo() {
        let e = json!({"objects": [cuadrado(1, 0.0, 0.0, 100.0, cmyk(1.0, 0.0, 0.0, 0.0)),
                                   cuadrado(2, 30.0, 30.0, 40.0, cmyk(0.0, 0.0, 1.0, 0.0))]});
        let t = traps(&atrapar_con(&e, &Opciones { modo: Modo::Invertido, ..reglas(0.1) }));
        assert_eq!(t[0]["name"], "trap C>Y 0.10");
        assert!(anillo(&t[0]).abs() < 1e-6, "el cian entra en el amarillo: el trap queda dentro del cuadrado");
        assert_eq!(leer_ajuste("trap K/Y 0.1"), Some(Ok(Some(Ajuste { izq: "K".into(), der: "Y".into(), rel: 'S', ancho_mm: 0.1 }))));
    }
}
