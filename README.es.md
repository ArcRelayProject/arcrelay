<p align="center"><img src="icons/icon.png" alt="Logotipo de ArcRelay" width="128"></p>

<h1 align="center">ArcRelay</h1>

<p align="center"><strong>Todos tus dispositivos, trabajando como uno solo.</strong></p>

<p align="center">Un espacio de trabajo de escritorio local-first para compartir contenido, entrada, archivos, impresiones y automatizaciones entre dispositivos.</p>

<p align="center">
  <a href="https://github.com/ArcRelayProject/arcrelay/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ArcRelayProject/arcrelay/ci.yml?branch=main&style=flat-square&label=build" alt="Estado de compilación"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/v/release/ArcRelayProject/arcrelay?include_prereleases&style=flat-square&label=release&color=7c5cff" alt="Última versión"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ArcRelayProject/arcrelay?style=flat-square&color=16a34a" alt="AGPL-3.0-only"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.de.md">Deutsch</a> · <a href="README.fr.md">Français</a> · Español · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center"><img src="assets/screenshots/quick-actions.png" alt="Acciones rápidas de ArcRelay" width="920"></p>

> Las capturas pertenecen a la interfaz de escritorio actual. Los dispositivos y la actividad mostrados son datos de ejemplo del modo de desarrollo.

## Funciones

ArcRelay descubre dispositivos en la red local y establece conexiones cifradas y autenticadas. La colaboración de escritorio no requiere una cuenta en la nube y solo expone las capacidades que apruebes.

| | Función | Uso |
| --- | --- | --- |
| ⚡ | Acciones rápidas | Buscar y abrir aplicaciones, carpetas, scripts y tareas frecuentes. |
| 🔁 | Automatización local | Combinar disparadores, condiciones, confirmaciones y pasos con un historial duradero. |
| 📋 | Portapapeles | Buscar y reutilizar texto e imágenes. |
| 📦 | Transferencia cercana | Enviar archivos directamente y de forma cifrada a dispositivos aprobados. |
| 📁 | Archivos remotos | Explorar carpetas autorizadas y subir o descargar archivos. |
| ⌨️ | Entrada entre pantallas | Mover el ratón y el teclado entre ordenadores mediante una disposición visual. |
| 🖨️ | Impresoras compartidas | Publicar impresoras locales y seguir trabajos remotos. |
| 🛡️ | Privacidad al presentar | Ocultar ventanas seleccionadas durante una presentación o duplicación de pantalla. |

## Capturas de pantalla

<table><tr>
<td width="50%"><img src="assets/screenshots/nearby-transfer.png" alt="Transferencia de archivos cercana"></td>
<td width="50%"><img src="assets/screenshots/input-workspace.png" alt="Entrada entre pantallas"></td>
</tr><tr>
<td align="center"><strong>Transferencia cercana</strong><br>Envío directo con progreso, verificación de integridad y controles de recepción.</td>
<td align="center"><strong>Entrada entre pantallas</strong><br>Gestión visual de pantallas, conexiones y diagnósticos.</td>
</tr></table>

## Privacidad y seguridad

- QUIC y TLS 1.3 protegen las conexiones entre dispositivos.
- El emparejamiento registra la identidad y los permisos de cada capacidad.
- Los paquetes de actualización se firman con la clave de Tauri updater y se verifican antes de instalarlos.
- Los paquetes oficiales de macOS usan una firma Developer ID y la notarización de Apple.
- Puedes informar de vulnerabilidades en privado según la [política de seguridad](SECURITY.md).

## Descarga y canales de actualización

| Canal | Público | Publicación |
| --- | --- | --- |
| **Stable** | Uso diario | Se publica cuando un mantenedor crea una etiqueta `vMAJOR.MINOR.PATCH`. |
| **Test** | Pruebas anticipadas | Se publica después de cada CI correcta en `main` y puede incluir cambios incompletos. |

Descarga ArcRelay desde **[GitHub Releases](https://github.com/ArcRelayProject/arcrelay/releases)**. Los destinos oficiales actuales son macOS para Apple Silicon e Intel, y Windows para x86-64 y ARM64. La aplicación móvil oficial se distribuye por separado y su código fuente no forma parte de este repositorio.

Elige Stable o Test en **Ajustes → General → Canal de actualización**. Ambos canales usan manifiestos HTTPS y la misma clave pública integrada. Publicar una versión de prueba no cambia el canal Stable.

## Ejecutar desde el código fuente

Necesitas Rust stable, Node.js 22 y las dependencias de desarrollo de Tauri 2 para tu plataforma.

```bash
git clone https://github.com/ArcRelayProject/arcrelay.git
cd arcrelay
npm ci
npm run tauri -- dev
```

Este repositorio contiene el host de Tauri, la interfaz Svelte, las integraciones de escritorio y la automatización de versiones. Aceptamos issues y Pull Requests de alcance claro. Lee [CONTRIBUTING.md](CONTRIBUTING.md) antes de contribuir.

## Licencia y marcas

Copyright © 2026 Shenzhen Changning Technology Co., Ltd.

El código fuente se ofrece bajo [GNU AGPL v3.0 only](LICENSE). Consulta [TRADEMARKS.md](TRADEMARKS.md) para el nombre ArcRelay, el logotipo y los demás recursos de marca.
