# Project KX27: paleta de trapping

Estado: primera integración, 2026-10-23. No cambia la calificación alfa del editor.

## Uso

1. Abre **Window / Ventana → Trapping**. También está en el menú del panel Pathfinder: **Trap…**.
2. Selecciona al menos dos trazados con rellenos planos distintos y sin contorno.
3. Define ancho (mm), dirección, tratamiento del negro y diferencia mínima; pulsa **Aplicar a la selección**.
4. La paleta permanece abierta. En **Pares de la selección**, ajusta ancho y dirección de cada par y pulsa **Actualizar par**.
5. Al recalcular, incluye el arte original y sus traps. **Restablecer pares** reemplaza los ajustes individuales con las reglas generales.

La paleta puede flotar o acoplarse como los demás paneles. Su ubicación se guarda con los espacios de trabajo; sus parámetros se serializan en las preferencias de interfaz. Los ajustes por par siguen representados en los nombres de los traps para mantener compatibilidad con el plugin. Aplicar o actualizar un par es un solo paso de deshacer.

**Localizar** selecciona un trap en el lienzo. Para volver a calcular, selecciona de nuevo el arte y sus traps. Los pares mostrados pertenecen a la selección actual.

## Arquitectura y procedencia

- `crates/trapping`: lógica independiente de interfaz adaptada de `kartonpapers-ai/steppa-trap` 0.4.0, `src/trap.rs`, blob `349ab24379f7e4b9577401555e5b17aa95553ae7` (Steppa; MIT OR Apache-2.0). Incluye sus 19 pruebas originales; añade tolerancia de redondeo f32 en los umbrales de negro del host. No incluye el wrapper ABI ni usa código inseguro.
- `crates/engine/src/cmd/trapping.rs`: `trapping.info` (consulta sin calcular geometría) y `trapping.apply` (validación, cálculo y edición transaccional).
- `crates/ui-egui/src/panels/trapping.rs`: controles, mensajes y ajustes de pares; llama a los mismos comandos que automatización.

Es el mismo motor para escritorio y WebAssembly. No hace falta instalar el `.wasm` del plugin. La integración usa el adaptador de objetos existente y no modifica el contrato de plugins.

## Límites conservados y protecciones

- Rellenos planos CMYK, RGB o gris. La conversión RGB es simple, sin perfil ICC.
- No admite tintas directas ni conserva vínculos a muestras. Esta primera integración rechaza rellenos vinculados a muestras; conviértelos a color de proceso.
- Rechaza contornos, transparencias, efectos y grupos con recorte o máscara en los objetos procesados. No calcula sobre texto, imágenes ni objetos vivos.
- El modo de negro **simula** sobreimpresión con un objeto de CMYK combinado. No activa el atributo PDF de sobreimpresión y no equivale a un RIP.
- Las curvas se convierten a segmentos con el método y los límites de subdivisión del plugin 0.4. No es un offset Bézier exacto.
- Máximo 80 trazados (incluidos traps) y 12.000 nodos de entrada por operación. El cálculo se realiza al pulsar un botón, no en cada frame.
- Incluye todo el arte que pueda ocultar un encuentro. El motor solo conoce el ámbito seleccionado; no hace una comprobación integral del documento.
- Los nombres que empiezan por `trap` se interpretan como controles, igual que en 0.4. Evita ese prefijo en el arte original.

Esta integración no se presenta como trapping profesional validado. Antes de producción, revisar separaciones y archivos exportados con casos de referencia de imprenta.

## Validación

```sh
cargo test -p vectorcraft-trapping
cargo test -p vectorcraft-engine --test trapping
cargo test -p vectorcraft-ui-egui --test trapping_panel
cargo check --target wasm32-unknown-unknown -p vectorcraft-web
cargo xtask ci
```

Capturas opcionales del panel real (requieren un adaptador gráfico disponible):

```sh
TRAPCOLOR_SCREENSHOTS=/tmp/trapcolor-panel cargo test -p vectorcraft-ui-egui --test trapping_panel trapping_panel_visual_capture
```

Cubren un panel poblado en tema oscuro de 340 px y claro de 280 px.

## Compilación web y GitHub Pages

El workflow **Project KX27 web** compila la aplicación completa con Trunk y deja un artefacto `trapcolor-web` en GitHub Actions. Los pushes a la rama de desarrollo solo generan el artefacto. Los pushes a `main` compilan y publican en GitHub Pages.

Para alojarla, habilita **Settings → Pages → Source: GitHub Actions** en el repositorio. Una vez que el workflow esté en la rama principal, ejecútalo con **Run workflow** y la opción **publish** activada. La acción de despliegue devuelve la URL real. La configuración inicial de Pages requiere permisos de administración y no puede activarse con el `GITHUB_TOKEN` normal del workflow.
