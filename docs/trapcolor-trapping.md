# Project KX27: paleta de trapping

Estado: análisis previo de encuentros y soporte inicial de spots vectoriales, 2026-10-24. No cambia la calificación alfa del editor.

## Uso

1. Abre **Window / Ventana → Trapping**. También está en el menú del panel Pathfinder: **Trap…**.
2. Selecciona al menos dos trazados con rellenos planos distintos (proceso o muestras spot) y sin contorno.
3. Define ancho (mm), dirección, tratamiento del negro y diferencia mínima. **Analizar contactos** permite revisar los encuentros previstos sin modificar el documento; **Aplicar a la selección** crea o recalcula los traps.
4. La paleta permanece abierta. En **Traps de la selección**, ajusta ancho y dirección de cada par y pulsa **Actualizar par**.
5. Al recalcular, incluye el arte original y sus traps. **Restablecer pares** reemplaza los ajustes individuales con las reglas generales.

La paleta puede flotar o acoplarse como los demás paneles. Su ubicación se guarda con los espacios de trabajo; sus parámetros se serializan en las preferencias de interfaz. Las reglas nuevas se guardan en los atributos `data-kx27-trap-v1-*` del objeto, separadas de los nombres visibles y asociadas al ID del trap. Acompañan las copias del objeto sin cambiar el formato nativo. Se leen también los nombres de controles del plugin 0.4; al recalcular se migran a metadatos. El plugin WASM original no interpreta estas reglas nuevas. Aplicar o actualizar un par es un solo paso de deshacer.

**Localizar** selecciona un trap en el lienzo. Para volver a calcular, selecciona de nuevo el arte y sus traps. Los pares mostrados pertenecen a la selección actual.

## Revisar antes de aplicar

**Análisis de contactos** muestra una fila por pareja de objetos fuente: nombres de los trazados, tintas, dirección, ancho y motivo (claridad, negro K, diferencia mínima o ajuste existente). Varios encuentros de las mismas tintas siguen compartiendo su regla y su trap; todavía no hay excepciones por borde ni edición local. Componentes separados entre los mismos dos objetos aparecen en una sola fila.

**Seleccionar pareja** selecciona los dos objetos fuente sin desplazar la vista. **Recuperar selección analizada** restaura el ámbito original; aplicar queda deshabilitado mientras se inspecciona una selección distinta. El informe persiste al cambiar la selección y se descarta al editar el documento, cambiar sus reglas, aplicar o cambiar de documento. Es temporal y no se guarda en preferencias ni en el archivo.

El informe y Aplicar comparten un plan geométrico puro. Analizar calcula recortes y offsets temporales para prever las mismas zonas, pero no materializa objetos, cambia la selección ni crea pasos de deshacer. Cero encuentros es un resultado válido de análisis. El ámbito, ancho y tolerancias del algoritmo determinan los encuentros previstos: puede incluir separaciones pequeñas dentro del ancho; no es un detector universal de adyacencia ni un motor completo de visibilidad. Es el primer paso de revisión separada, no la implementación completa de las cuatro etapas profesionales.

## Arquitectura y procedencia

- `crates/trapping`: lógica independiente de interfaz adaptada de `kartonpapers-ai/steppa-trap` 0.4.0, `src/trap.rs`, blob `349ab24379f7e4b9577401555e5b17aa95553ae7` (Steppa; MIT OR Apache-2.0). Conserva las 19 pruebas originales y añade pruebas de identidad spot, porcentajes y claves de pares. Añade tolerancia de redondeo f32 en los umbrales de negro del host. No incluye el wrapper ABI ni usa código inseguro.
- `crates/engine/src/cmd/trapping.rs`: `trapping.info` (consulta sin calcular geometría), `trapping.analyze` (plan de encuentros de solo lectura) y `trapping.apply` (validación, cálculo y edición transaccional).
- `crates/ui-egui/src/panels/trapping.rs`: controles, mensajes y ajustes de pares; llama a los mismos comandos que automatización.

Es el mismo motor para escritorio y WebAssembly. No hace falta instalar el `.wasm` del plugin. La integración usa el adaptador de objetos existente y no modifica el contrato de plugins.

## Límites conservados y protecciones

- Rellenos planos CMYK, RGB, gris, Lab y muestras globales o spot. RGB conserva la conversión simple del plugin; Lab de proceso pasa a CMYK con la configuración de color activa.
- Las muestras spot conservan nombre, vínculo y porcentaje. Su alternativo Lab/CMYK sirve para claridad y pantalla, nunca se suma como tinta CMYK. Dos spots distintos con el mismo alternativo siguen siendo dos planchas. Registro y nombres de colorante reservados por PDF (All, None, Cyan, Magenta, Yellow, Black) se rechazan. No se incluye una biblioteca de valores Pantone.
- Rechaza contornos, transparencias, efectos y grupos con recorte o máscara en los objetos procesados. No calcula sobre texto, imágenes ni objetos vivos.
- Los traps llevan rellenos independientes en sobreimpresión: un CMYK de máximos por canal y un relleno por spot presente, con el máximo porcentaje de esa tinta. El exportador existente escribe spots como `Separation` y preserva `OP/op` y `OPM`; no se añade una pintura DeviceN al modelo. Exporta con sobreimpresión **Preserve**, sin convertir spots a proceso. View → Overprint Preview aproxima las mezclas en pantalla con Multiply; no es una prueba colorimétrica ni equivale a un RIP.
- Las curvas se convierten a segmentos con el método y los límites de subdivisión del plugin 0.4. No es un offset Bézier exacto.
- Máximo 80 trazados (incluidos traps) y 12.000 nodos de entrada por operación. El cálculo se realiza al pulsar un botón, no en cada frame.
- Incluye todo el arte que pueda ocultar un encuentro. El motor solo conoce el ámbito seleccionado; no hace una comprobación integral del documento.
- Los nombres heredados que empiezan por `trap` se interpretan como controles. Las reglas nuevas se consultan por ID, aunque cambies el nombre visible del trap. Renombrar una muestra spot cambia su identidad de par: al recalcular se aplican reglas nuevas. Revisa los ajustes después de renombrar, duplicar o copiar traps entre documentos.

Blanco opaco, metálicos, barnices y otras tintas técnicas requieren reglas y datos de impresión adicionales; esta etapa no los valida. Se rechazan spots con alternativo blanco a porcentaje positivo. Esta integración no se presenta como trapping profesional validado. Antes de producción, revisar separaciones y archivos exportados con casos de referencia de imprenta.

## Comprobar un par spot

1. Crea una muestra global de tipo Spot (por ejemplo `Tinta prueba A`) y aplica 40 % a un trazado.
2. Hazlo coincidir con un trazado CMYK u otra muestra Spot; selecciona ambos.
3. Aplica Trapping y comprueba los nombres y porcentajes en Appearance. Los rellenos del trap deben marcar sobreimpresión.
4. Guarda y vuelve a abrir el documento nativo; ajusta el ancho de ese par y comprueba deshacer. Conserva el nativo para recalcular: un PDF de producción sin datos de edición no guarda todas las reglas.
5. Exporta PDF con Advanced → Overprint: Preserve y conserva las tintas directas en Output. Verifica las planchas con un RIP de referencia antes de producción.

Referencias públicas de formato: [ISO 32000, espacios de color](https://pdf-issues.pdfa.org/32000-2-2020/clause08.html) y [planos CMYK/spot de referencia](https://ghostscript.readthedocs.io/en/latest/Devices.html#tiff).

## Validación

```sh
cargo test -p vectorcraft-trapping
cargo test -p vectorcraft-engine --test trapping
cargo test -p vectorcraft-ui-egui --test trapping_panel
cargo check --target wasm32-unknown-unknown -p vectorcraft-web
cargo xtask ci
```

Comprobado el 2026-10-24: 24 pruebas de geometría, 14 de motor y 5 de paleta aprobadas; capturas nativas en ambos temas, comprobación WASM, formato, assets, brands y layers aprobados. `cargo xtask ci` se detiene en avisos previos de Clippy en `render/src/fx.rs` e `pdf/src/import_scan.rs`; no representa una ejecución completa de todos los tests del workspace. La comprobación específica de UI también encuentra avisos previos fuera de esta paleta (Pathfinder y thumbnail worker).

Capturas opcionales del panel real (requieren un adaptador gráfico disponible):

```sh
TRAPCOLOR_SCREENSHOTS=/tmp/trapcolor-panel cargo test -p vectorcraft-ui-egui --test trapping_panel trapping_panel_visual_capture
```

Cubren un panel con análisis y traps en tema oscuro de 340 px y claro de 280 px.

## Compilación web y GitHub Pages

El workflow **Project KX27 web** compila la aplicación completa con Trunk y deja un artefacto `trapcolor-web` en GitHub Actions. Los pushes a la rama de desarrollo solo generan el artefacto. Los pushes a `main` compilan y publican en GitHub Pages.

Para alojarla, habilita **Settings → Pages → Source: GitHub Actions** en el repositorio. Una vez que el workflow esté en la rama principal, ejecútalo con **Run workflow** y la opción **publish** activada. La acción de despliegue devuelve la URL real. La configuración inicial de Pages requiere permisos de administración y no puede activarse con el `GITHUB_TOKEN` normal del workflow.
