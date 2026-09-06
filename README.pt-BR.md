<p align="center"><img src="icons/icon.png" alt="Logotipo do ArcRelay" width="128"></p>

<h1 align="center">ArcRelay</h1>

<p align="center"><strong>Todos os seus dispositivos trabalhando como um só.</strong></p>

<p align="center">Um espaço de trabalho desktop local-first para compartilhar conteúdo, entrada, arquivos, impressões e automações entre dispositivos.</p>

<p align="center">
  <a href="https://github.com/ArcRelayProject/arcrelay/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ArcRelayProject/arcrelay/ci.yml?branch=main&style=flat-square&label=build" alt="Status do build"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/v/release/ArcRelayProject/arcrelay?include_prereleases&style=flat-square&label=release&color=7c5cff" alt="Versão mais recente"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ArcRelayProject/arcrelay?style=flat-square&color=16a34a" alt="AGPL-3.0-only"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.de.md">Deutsch</a> · <a href="README.fr.md">Français</a> · <a href="README.es.md">Español</a> · Português
</p>

<p align="center"><img src="assets/screenshots/quick-actions.png" alt="Ações rápidas do ArcRelay" width="920"></p>

> As capturas mostram a interface desktop atual. Os dispositivos e as atividades são dados de exemplo do modo de desenvolvimento.

## Recursos

O ArcRelay encontra dispositivos na rede local e estabelece conexões autenticadas e criptografadas. A colaboração entre computadores não exige uma conta na nuvem e só expõe as capacidades que você autorizar.

| | Recurso | Finalidade |
| --- | --- | --- |
| ⚡ | Ações rápidas | Pesquisar e abrir aplicativos, pastas, scripts e tarefas frequentes. |
| 🔁 | Automação local | Combinar gatilhos, condições, confirmações e etapas com histórico persistente. |
| 📋 | Área de transferência | Pesquisar e reutilizar texto e imagens. |
| 📦 | Transferência próxima | Enviar arquivos diretamente e com criptografia para dispositivos aprovados. |
| 📁 | Arquivos remotos | Navegar por pastas autorizadas, enviar e baixar arquivos. |
| ⌨️ | Entrada entre telas | Mover mouse e teclado entre computadores com base em uma disposição visual. |
| 🖨️ | Compartilhamento de impressora | Publicar impressoras locais e acompanhar trabalhos remotos. |
| 🛡️ | Privacidade em apresentações | Ocultar janelas selecionadas durante o compartilhamento de tela. |

## Capturas de tela

<table><tr>
<td width="50%"><img src="assets/screenshots/nearby-transfer.png" alt="Transferência de arquivos próxima"></td>
<td width="50%"><img src="assets/screenshots/input-workspace.png" alt="Entrada entre telas"></td>
</tr><tr>
<td align="center"><strong>Transferência próxima</strong><br>Envio direto com progresso, verificação de integridade e controles de recebimento.</td>
<td align="center"><strong>Entrada entre telas</strong><br>Gerenciamento visual de telas, conexões e diagnósticos.</td>
</tr></table>

## Privacidade e segurança

- QUIC e TLS 1.3 protegem as conexões entre dispositivos.
- O pareamento registra a identidade e permissões separadas para cada capacidade.
- Os pacotes de atualização são assinados com a chave do Tauri updater e verificados antes da instalação.
- Os pacotes oficiais para macOS recebem assinatura Developer ID e notarização da Apple.
- Vulnerabilidades podem ser relatadas em particular conforme a [política de segurança](SECURITY.md).

## Download e canais de atualização

| Canal | Público | Publicação |
| --- | --- | --- |
| **Stable** | Uso diário | Publicado quando um mantenedor cria uma tag `vMAJOR.MINOR.PATCH`. |
| **Test** | Testes antecipados | Publicado após cada CI bem-sucedida em `main` e pode incluir mudanças ainda incompletas. |

Baixe o ArcRelay em **[GitHub Releases](https://github.com/ArcRelayProject/arcrelay/releases)**. Os alvos oficiais atuais são macOS em Apple Silicon e Windows x86-64. O aplicativo móvel oficial é distribuído separadamente e seu código-fonte não faz parte deste repositório.

Escolha Stable ou Test em **Configurações → Geral → Canal de atualização**. Os dois canais usam manifestos HTTPS e a mesma chave pública embutida. Uma versão de teste não altera o canal Stable.

## Executar a partir do código-fonte

Requisitos: Rust stable, Node.js 22 e as dependências de desenvolvimento do Tauri 2 para a plataforma.

```bash
git clone https://github.com/ArcRelayProject/arcrelay.git
cd arcrelay
npm ci
npm run tauri -- dev
```

Este repositório contém o host Tauri, a interface Svelte, as integrações desktop e a automação de releases. Issues e Pull Requests com escopo claro são bem-vindos. Leia [CONTRIBUTING.md](CONTRIBUTING.md) antes de contribuir.

## Licença e marcas

Copyright © 2026 Shenzhen Changning Technology Co., Ltd.

O código-fonte é distribuído sob a [GNU AGPL v3.0 only](LICENSE). Consulte [TRADEMARKS.md](TRADEMARKS.md) sobre o nome ArcRelay, o logotipo e outros ativos da marca.
