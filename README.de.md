<p align="center"><img src="icons/icon.png" alt="ArcRelay-Logo" width="128"></p>

<h1 align="center">ArcRelay</h1>

<p align="center"><strong>Alle Geräte in einem Arbeitsbereich.</strong></p>

<p align="center">Ein lokaler Desktop-Arbeitsbereich für Inhalte, Eingaben, Dateien, Druckaufträge und wiederkehrende Abläufe auf mehreren Geräten.</p>

<p align="center">
  <a href="https://github.com/ArcRelayProject/arcrelay/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ArcRelayProject/arcrelay/ci.yml?branch=main&style=flat-square&label=build" alt="Build-Status"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/v/release/ArcRelayProject/arcrelay?include_prereleases&style=flat-square&label=release&color=7c5cff" alt="Aktuelle Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ArcRelayProject/arcrelay?style=flat-square&color=16a34a" alt="AGPL-3.0-only"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · Deutsch · <a href="README.fr.md">Français</a> · <a href="README.es.md">Español</a> · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center"><img src="assets/screenshots/quick-actions.png" alt="ArcRelay-Schnellaktionen" width="920"></p>

> Die Aufnahmen zeigen die aktuelle ArcRelay-Desktopoberfläche. Geräte und Aktivitäten sind Beispieldaten aus dem Entwicklungsmodus.

## Funktionen

ArcRelay findet Geräte im lokalen Netzwerk und stellt authentifizierte, verschlüsselte Verbindungen her. Für die Desktop-Zusammenarbeit ist kein Cloud-Konto erforderlich. Andere Geräte erhalten nur die von dir freigegebenen Rechte.

| | Funktion | Zweck |
| --- | --- | --- |
| ⚡ | Schnellaktionen | Apps, Ordner, Skripte und häufige Aufgaben suchen und starten. |
| 🔁 | Lokale Automatisierung | Auslöser, Bedingungen, Bestätigungen und geordnete Schritte mit Verlauf verbinden. |
| 📋 | Zwischenablage | Text und Bilder suchen und wiederverwenden. |
| 📦 | Übertragung in der Nähe | Dateien direkt und verschlüsselt an freigegebene Geräte senden. |
| 📁 | Entfernte Dateien | Freigegebene Ordner durchsuchen sowie Dateien hoch- und herunterladen. |
| ⌨️ | Geräteübergreifende Eingabe | Maus und Tastatur anhand einer visuellen Bildschirm-Anordnung zwischen Computern bewegen. |
| 🖨️ | Druckerfreigabe | Lokale Drucker bereitstellen und entfernte Aufträge verfolgen. |
| 🛡️ | Präsentationsschutz | Ausgewählte App-Fenster bei einer Bildschirmfreigabe abdecken. |

## Bildschirmaufnahmen

<table><tr>
<td width="50%"><img src="assets/screenshots/nearby-transfer.png" alt="Dateiübertragung in der Nähe"></td>
<td width="50%"><img src="assets/screenshots/input-workspace.png" alt="Geräteübergreifende Eingabe"></td>
</tr><tr>
<td align="center"><strong>Übertragung in der Nähe</strong><br>Direkte Übertragung mit Fortschritt, Integritätsprüfung und Empfangsregeln.</td>
<td align="center"><strong>Geräteübergreifende Eingabe</strong><br>Bildschirme, Verbindungen und Diagnosen visuell verwalten.</td>
</tr></table>

## Datenschutz und Sicherheit

- QUIC und TLS 1.3 schützen Geräteverbindungen.
- Die Kopplung speichert Geräteidentität und einzelne Berechtigungen.
- Aktualisierungspakete werden mit dem Tauri-Updater-Schlüssel signiert und vor der Installation geprüft.
- Offizielle macOS-Pakete tragen eine Developer-ID-Signatur und werden von Apple notarisiert.
- Sicherheitsprobleme können gemäß der [Sicherheitsrichtlinie](SECURITY.md) vertraulich gemeldet werden.

## Download und Update-Kanäle

| Kanal | Zielgruppe | Veröffentlichung |
| --- | --- | --- |
| **Stable** | Tägliche Nutzung | Wird durch ein `vMAJOR.MINOR.PATCH`-Tag eines Maintainers veröffentlicht. |
| **Test** | Frühes Testen | Wird nach jedem erfolgreichen CI-Lauf auf `main` automatisch veröffentlicht und kann unfertige Änderungen enthalten. |

Downloads findest du unter **[GitHub Releases](https://github.com/ArcRelayProject/arcrelay/releases)**. Aktuell werden macOS auf Apple Silicon und Windows x86-64 offiziell unterstützt. Die offizielle mobile App wird separat vertrieben; ihr Quellcode gehört nicht zu diesem Repository.

Unter **Einstellungen → Allgemein → Update-Kanal** kannst du Stable oder Test wählen. Beide Kanäle verwenden HTTPS-Manifeste und denselben eingebetteten öffentlichen Schlüssel. Eine Test-Veröffentlichung verändert den Stable-Kanal nicht.

## Aus dem Quellcode starten

Erforderlich sind Rust stable, Node.js 22 und die Plattform-Abhängigkeiten für Tauri 2.

```bash
git clone https://github.com/ArcRelayProject/arcrelay.git
cd arcrelay
npm ci
npm run tauri -- dev
```

Dieses Repository enthält den Tauri-Host, die Svelte-Oberfläche, Desktop-Integrationen und die Release-Automatisierung. Issues und klar abgegrenzte Pull Requests sind willkommen. Lies zuvor [CONTRIBUTING.md](CONTRIBUTING.md).

## Lizenz und Marken

Copyright © 2026 Shenzhen Changning Technology Co., Ltd.

Der Quellcode steht unter [GNU AGPL v3.0 only](LICENSE). Hinweise zum Namen ArcRelay, zum Logo und zu weiteren Markeninhalten enthält [TRADEMARKS.md](TRADEMARKS.md).
