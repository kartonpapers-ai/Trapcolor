# Project KX27: paleta de trapping

Estado: primera etapa de sólido hacia imagen PNG/JPEG opaca incrustada; degradados lineales y spots sólidos conservados, 2026-10-25. No cambia la calificación alfa del editor.

## Uso

1. Abre **Window / Ventana → Trapping**. También está en el menú del panel Pathfinder: **Trap…**.
2. Selecciona trazados sin contorno: sólidos de proceso/spot, sólido contra degradado lineal opaco o sólido contra imagen PNG/JPEG opaca incrustada.
3. Define ancho (mm), dirección, tratamiento del negro y diferencia mínima. **Analizar contactos** permite revisar los encuentros previstos sin modificar el documento; **Aplicar a la selección** crea o recalcula los traps.
4. La paleta permanece abierta. En **Traps de la selección**, ajusta ancho y dirección de cada par y pulsa **Actualizar par**.
5. Al recalcular, incluye el arte original y sus traps. **Restablecer pares** reemplaza los ajustes individuales con las reglas generales.

La paleta puede flotar o acoplarse como los demás paneles. Su ubicación se guarda con los espacios de trabajo; sus parámetros se serializan en las preferencias de interfaz. Las reglas nuevas se guardan en los atributos `data-kx27-trap-v1-*` del objeto, separadas de los nombres visibles y asociadas al ID del trap. Acompañan las copias del objeto sin cambiar el formato nativo. Se leen también los nombres de controles del plugin 0.4; al recalcular se migran a metadatos. El plugin WASM original no interpreta estas reglas nuevas. Aplicar o actualizar un par es un solo paso de deshacer.

**Localizar** selecciona un trap en el lienzo. Para volver a calcular, selecciona de nuevo el arte y sus traps. Los pares mostrados pertenecen a la selección actual.

## Revisar antes de aplicar

**Análisis de contactos** muestra una fila por pareja de objetos fuente: nombres de los trazados, tintas, dirección, ancho y motivo (claridad, negro K, diferencia mínima o ajuste existente). Los encuentros sólido/sólido de las mismas tintas comparten regla y trap; los de degradado se asocian al objeto de degradado y al color sólido; todavía no hay excepciones por borde ni edición local. Componentes separados entre los mismos dos objetos aparecen en una sola fila.

**Seleccionar pareja** selecciona los dos objetos fuente sin desplazar la vista. **Recuperar selección analizada** restaura el ámbito original; aplicar queda deshabilitado mientras se inspecciona una selección distinta. El informe persiste al cambiar la selección y se descarta al editar el documento, cambiar sus reglas, aplicar o cambiar de documento. Es temporal y no se guarda en preferencias ni en el archivo.

El informe y Aplicar comparten un plan geométrico puro. Analizar calcula recortes y offsets temporales para prever las mismas zonas, pero no materializa objetos, cambia la selección ni crea pasos de deshacer. Cero encuentros es un resultado válido de análisis. El ámbito, ancho y tolerancias del algoritmo determinan los encuentros previstos: puede incluir separaciones pequeñas dentro del ancho; no es un detector universal de adyacencia ni un motor completo de visibilidad. Es el primer paso de revisión separada, no la implementación completa de las cuatro etapas profesionales.

## Regla inicial: sólido hacia degradado

En un encuentro entre un sólido y un degradado lineal opaco, el sólido se expande hacia el degradado con el ancho elegido durante toda la unión. El trap lleva solo la tinta del sólido, en sobreimpresión, sin degradado propio. Esta regla fija prevalece sobre dirección general (automática, invertida o centrada), diferencia mínima y tratamiento de negro. En el par puedes cambiar ancho o elegir **Sin trap**; no invertir, centrar ni sobreimprimir toda la superficie del negro.

Los cuatro casos iniciales son amarillo hacia degradado cian, amarillo hacia degradado magenta, magenta hacia degradado cian y cian hacia degradado magenta. Se prueban con ambos órdenes de apilado. El motor usa la geometría visible y no toca las paradas, ubicación, porcentajes ni pintura de los originales. El blanco de proceso al 0 % se admite como parada opaca: no equivale a una parada transparente. No se detecta ni invierte la dirección por claridad local; tampoco se decide por un color promedio.

**Analizar contactos** identifica la regla fija. Cada degradado utiliza una identidad interna por ID de objeto, mostrada como **Degradado · nombre del objeto**; no se muestran claves internas. El trap conserva su ID, ancho y exclusión al recalcular con el mismo arte. Duplicar el degradado crea otra identidad y requiere revisar sus ajustes. Guarda el archivo nativo para conservar las reglas; importar un PDF de producción no sustituye ese archivo.

Admite entre 2 y 256 paradas ordenadas de proceso y plenamente opacas, con geometría válida. Un encuentro visible entre dos degradados se rechaza de forma atómica; varios degradados separados en la misma selección sí son posibles. Los degradados opacos también ocultan los encuentros que queden debajo. La regla no introduce rasterización. El RGB del SVG conserva la conversión simple existente; el PDF CMYK utiliza sus tintas de proceso. No se garantiza idéntica separación a partir de RGB y CMYK que solo se parezcan en pantalla.

## Primera etapa de imágenes: sólido hacia marco opaco

Coloca una imagen con **File → Place**, sin Link (o incrústala antes). Selecciona la imagen y el sólido, sin textos ni otros objetos vivos; usa **Analizar contactos** y luego **Aplicar a la selección**. Se admiten PNG y JPEG incrustados, con todos sus píxeles plenamente opacos. Un PNG con canal alfa también sirve si todos sus valores son exactamente opacos; se comprueban tanto alfa de 8 bits como de 16 bits. El blanco dentro de una foto opaca sigue perteneciendo a su marco.

El sólido entra en la imagen con ancho nominal constante, solo su tinta y sobreimpresión. Esta regla fija prevalece sobre dirección global, diferencia mínima y tratamiento de negro; el par permite ancho o **Sin trap**. El informe dice **Sólido hacia imagen opaca** y muestra **Imagen · nombre**. La identidad interna se asocia al ID de imagen; duplicarla crea un par nuevo que requiere revisión.

La imagen conserva tipo de objeto, ID, píxeles codificados, perfil embebido, dimensiones, transformación y lugar de apilado. El cálculo recibe exclusivamente un polígono temporal de cuatro esquinas, transformadas con la matriz real de la imagen: también admite giro, espejo e inclinación, sin sustituirlos por una caja alineada a los ejes. Los traps nuevos quedan encima del arte seleccionado; los ya existentes mantienen su ubicación. La integración no remuestrea, vectoriza ni reescribe la imagen. La exportación sigue a cargo del motor PDF existente; se comprueban las referencias de imagen y la sobreimpresión del trap.

Esta etapa **no lee los colores del borde para decidir la dirección**, no sigue siluetas, no compensa transparencias y no calcula imagen contra imagen ni imagen contra degradado. Estos encuentros visibles se rechazan antes de editar. Una imagen opaca sí participa como oclusor de encuentros inferiores. Se rechazan imágenes vinculadas o representadas solo por su vista previa, máscaras, recortes, efectos, apariencias añadidas, calco y perspectiva vinculada. No incluye TIFF ni otros formatos en esta primera etapa.

El examen de opacidad se hace al analizar o aplicar, nunca en cada frame. Por selección: hasta **32 megapíxeles y 64 MB codificados de imágenes distintas**, con decodificación limitada a 128 MB por imagen; una imagen compartida por varios objetos se comprueba una vez. Se valida formato, dimensiones y transformación antes de crear geometría. Datos ausentes, corruptos, excesivos o con transparencia fallan sin cambios de documento, selección ni historial.

Los filtros de plugins siguen recibiendo solo trazados. El adaptador nativo puede conservar referencias de imagen como `{id}` en el orden de pintado; el adaptador de salida exige que esas referencias no incluyan cambios de geometría, nombre, rellenos ni opacidad. Esto evita convertir accidentalmente una imagen en un trazado vacío. No se guardan nuevos campos en el formato nativo.

## Arquitectura y procedencia

- `crates/trapping`: lógica independiente de interfaz adaptada de `kartonpapers-ai/steppa-trap` 0.4.0, `src/trap.rs`, blob `349ab24379f7e4b9577401555e5b17aa95553ae7` (Steppa; MIT OR Apache-2.0). Conserva las 19 pruebas originales y añade pruebas de identidad spot, porcentajes y claves de pares. Añade tolerancia de redondeo f32 en los umbrales de negro del host. No incluye el wrapper ABI ni usa código inseguro.
- `crates/engine/src/cmd/trapping.rs`: `trapping.info` (consulta sin calcular geometría), `trapping.analyze` (plan de encuentros de solo lectura) y `trapping.apply` (validación, cálculo y edición transaccional).
- `crates/ui-egui/src/panels/trapping.rs`: controles, mensajes y ajustes de pares; llama a los mismos comandos que automatización.

Es el mismo motor para escritorio y WebAssembly. No hace falta instalar el `.wasm` del plugin. La integración usa el adaptador de objetos existente y no modifica el contrato de plugins.

## Límites conservados y protecciones

- Rellenos sólidos CMYK, RGB, gris, Lab y muestras globales o spot. También sólido contra degradado lineal opaco de proceso. RGB conserva la conversión simple del plugin; Lab de proceso pasa a CMYK con la configuración de color activa.
- Las muestras spot conservan nombre, vínculo y porcentaje. Su alternativo Lab/CMYK sirve para claridad y pantalla, nunca se suma como tinta CMYK. Dos spots distintos con el mismo alternativo siguen siendo dos planchas. Registro y nombres de colorante reservados por PDF (All, None, Cyan, Magenta, Yellow, Black) se rechazan. No se incluye una biblioteca de valores Pantone.
- Rechaza contornos, transparencias, efectos y grupos con recorte o máscara en los objetos procesados. Los degradados radiales/libres, los vinculados a una muestra de degradado y los degradados con paradas spot se rechazan en esta etapa. No calcula sobre texto ni objetos vivos. Las imágenes solo se admiten en la etapa acotada descrita a continuación.
- Los traps llevan rellenos independientes en sobreimpresión: un CMYK de máximos por canal y un relleno por spot presente, con el máximo porcentaje de esa tinta. El exportador existente escribe spots como `Separation` y preserva `OP/op` y `OPM`; no se añade una pintura DeviceN al modelo. Exporta con sobreimpresión **Preserve**, sin convertir spots a proceso. View → Overprint Preview aproxima las mezclas en pantalla con Multiply; no es una prueba colorimétrica ni equivale a un RIP.
- Las curvas se convierten a segmentos con el método y los límites de subdivisión del plugin 0.4. No es un offset Bézier exacto.
- Máximo 80 objetos (incluidos traps) y 12.000 nodos de entrada por operación. El cálculo se realiza al pulsar un botón, no en cada frame.
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

Comprobado el 2026-10-25: 26 pruebas de geometría, 24 de motor y 7 de paleta aprobadas; capturas nativas en ambos temas, comprobación WASM, formato, assets, brands y layers aprobados. `cargo xtask ci` se detiene en avisos previos de Clippy en `render/src/fx.rs` e `pdf/src/import_scan.rs`; no representa una ejecución completa de todos los tests del workspace. La comprobación específica de UI también encuentra avisos previos fuera de esta paleta (Pathfinder y thumbnail worker).

Además pasaron las 12 pruebas unitarias de plugins y las 10 de su ABI. Las pruebas temporales de ABI se ejecutaron con `wasmi` optimizado (`cargo test -p vectorcraft-plugins --test abi --config 'profile.dev.package.wasmi.opt-level=3' -- --test-threads=1`). Las capturas nativas cubren sólido/imagen en 280 px claro y 340 px oscuro. Una comprobación local con Ghostscript `tiffsep` a 600 dpi sobre una imagen sintética confirmó amarillo al 100 % dentro del trap a tres alturas y la presencia de las otras planchas de la imagen; no sustituye validación de producción.

Los archivos de ejemplo SVG y PDF del usuario se comprobaron localmente, sin incorporarlos al repositorio: cuatro encuentros por archivo y fuentes sin cambios. Las pruebas sintéticas de los cuatro casos cubren ambos apilados, ancho y tinta exactos, sobreimpresión, guardado, deshacer/rehacer, exclusión y entradas no admitidas. Las separaciones del PDF se inspeccionaron además con Ghostscript `tiffsep` a 600 dpi: la franja añade la tinta sólida al 100 % en tres alturas de cada encuentro, conservando las demás tintas dentro del redondeo de 8 bits observado (hasta 0,4 puntos porcentuales). Es una comprobación técnica local, no validación en máquina de impresión.

Capturas opcionales del panel real (requieren un adaptador gráfico disponible):

```sh
TRAPCOLOR_SCREENSHOTS=/tmp/trapcolor-panel cargo test -p vectorcraft-ui-egui --test trapping_panel trapping_panel_visual_capture
```

Cubren un panel con análisis y traps en tema oscuro de 340 px y claro de 280 px. Añade `KX27_GRADIENT_CAPTURE=1` para mostrar un par sólido/degradado o `KX27_IMAGE_CAPTURE=1` para sólido/imagen.

## Compilación web y GitHub Pages

El workflow **Project KX27 web** compila la aplicación completa con Trunk y deja un artefacto `trapcolor-web` en GitHub Actions. Los pushes a la rama de desarrollo solo generan el artefacto. Los pushes a `main` compilan y publican en GitHub Pages.

Para alojarla, habilita **Settings → Pages → Source: GitHub Actions** en el repositorio. Una vez que el workflow esté en la rama principal, ejecútalo con **Run workflow** y la opción **publish** activada. La acción de despliegue devuelve la URL real. La configuración inicial de Pages requiere permisos de administración y no puede activarse con el `GITHUB_TOKEN` normal del workflow.
