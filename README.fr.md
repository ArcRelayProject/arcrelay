<p align="center"><img src="icons/icon.png" alt="Logo ArcRelay" width="128"></p>

<h1 align="center">ArcRelay</h1>

<p align="center"><strong>Tous vos appareils, dans un même espace de travail.</strong></p>

<p align="center">Un espace de travail de bureau local-first pour transférer contenus, saisies, fichiers, impressions et automatisations entre vos appareils.</p>

<p align="center">
  <a href="https://github.com/ArcRelayProject/arcrelay/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ArcRelayProject/arcrelay/ci.yml?branch=main&style=flat-square&label=build" alt="État du build"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/v/release/ArcRelayProject/arcrelay?include_prereleases&style=flat-square&label=release&color=7c5cff" alt="Dernière version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ArcRelayProject/arcrelay?style=flat-square&color=16a34a" alt="AGPL-3.0-only"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.de.md">Deutsch</a> · Français · <a href="README.es.md">Español</a> · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center"><img src="assets/screenshots/quick-actions.png" alt="Actions rapides ArcRelay" width="920"></p>

> Les captures proviennent de l’interface de bureau actuelle. Les appareils et activités affichés sont des données d’exemple du mode développement.

## Fonctionnalités

ArcRelay détecte les appareils sur le réseau local et établit des connexions chiffrées et authentifiées. La collaboration entre ordinateurs ne demande aucun compte cloud, et seules les capacités que vous autorisez sont exposées.

| | Fonction | Usage |
| --- | --- | --- |
| ⚡ | Actions rapides | Rechercher et lancer des applications, dossiers, scripts et tâches fréquentes. |
| 🔁 | Automatisation locale | Associer déclencheurs, conditions, confirmations et étapes, avec un historique durable. |
| 📋 | Presse-papiers | Rechercher et réutiliser du texte et des images. |
| 📦 | Transfert à proximité | Envoyer directement des fichiers chiffrés aux appareils approuvés. |
| 📁 | Fichiers distants | Parcourir les dossiers autorisés, téléverser et télécharger des fichiers. |
| ⌨️ | Saisie multi-écrans | Déplacer souris et clavier entre ordinateurs selon une disposition visuelle. |
| 🖨️ | Partage d’imprimante | Publier une imprimante locale et suivre les travaux distants. |
| 🛡️ | Confidentialité en présentation | Masquer certaines fenêtres pendant un partage d’écran. |

## Captures d’écran

<table><tr>
<td width="50%"><img src="assets/screenshots/nearby-transfer.png" alt="Transfert de fichiers à proximité"></td>
<td width="50%"><img src="assets/screenshots/input-workspace.png" alt="Saisie multi-écrans"></td>
</tr><tr>
<td align="center"><strong>Transfert à proximité</strong><br>Envoi direct avec progression, contrôle d’intégrité et règles de réception.</td>
<td align="center"><strong>Saisie multi-écrans</strong><br>Gestion visuelle des écrans, connexions et diagnostics.</td>
</tr></table>

## Confidentialité et sécurité

- QUIC et TLS 1.3 protègent les connexions entre appareils.
- L’association enregistre l’identité des appareils et les autorisations par capacité.
- Les mises à jour sont signées avec la clé Tauri updater et vérifiées avant installation.
- Les paquets macOS officiels portent une signature Developer ID et sont notariés par Apple.
- Les vulnérabilités peuvent être signalées en privé selon la [politique de sécurité](SECURITY.md).

## Téléchargement et canaux de mise à jour

| Canal | Public | Publication |
| --- | --- | --- |
| **Stable** | Usage quotidien | Publié lorsqu’un mainteneur crée un tag `vMAJOR.MINOR.PATCH`. |
| **Test** | Évaluation anticipée | Publié après chaque CI réussie sur `main` et susceptible de contenir des changements inachevés. |

Téléchargez ArcRelay depuis **[GitHub Releases](https://github.com/ArcRelayProject/arcrelay/releases)**. Les cibles officielles actuelles sont macOS sur Apple Silicon et Windows x86-64. L’application mobile officielle est distribuée séparément et son code source ne fait pas partie de ce dépôt.

Choisissez Stable ou Test dans **Réglages → Général → Canal de mise à jour**. Les deux canaux utilisent des manifestes HTTPS et la même clé publique intégrée. Une version de test ne modifie jamais le canal Stable.

## Exécuter depuis les sources

Prérequis : Rust stable, Node.js 22 et les dépendances de développement de Tauri 2 pour votre plateforme.

```bash
git clone https://github.com/ArcRelayProject/arcrelay.git
cd arcrelay
npm ci
npm run tauri -- dev
```

Ce dépôt contient l’hôte Tauri, l’interface Svelte, les intégrations de bureau et l’automatisation des versions. Les issues et Pull Requests ciblées sont bienvenues. Consultez [CONTRIBUTING.md](CONTRIBUTING.md) avant de contribuer.

## Licence et marques

Copyright © 2026 Shenzhen Changning Technology Co., Ltd.

Le code source est publié sous [GNU AGPL v3.0 only](LICENSE). Consultez [TRADEMARKS.md](TRADEMARKS.md) pour le nom ArcRelay, son logo et les autres éléments de marque.
