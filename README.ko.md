<p align="center"><img src="icons/icon.png" alt="ArcRelay 로고" width="128"></p>

<h1 align="center">ArcRelay</h1>

<p align="center"><strong>여러 기기를 하나의 작업 환경처럼.</strong></p>

<p align="center">콘텐츠, 입력, 파일, 인쇄 작업과 반복 동작을 기기 사이에서 다루는 로컬 우선 데스크톱 작업 공간입니다.</p>

<p align="center">
  <a href="https://github.com/ArcRelayProject/arcrelay/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ArcRelayProject/arcrelay/ci.yml?branch=main&style=flat-square&label=build" alt="빌드 상태"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/v/release/ArcRelayProject/arcrelay?include_prereleases&style=flat-square&label=release&color=7c5cff" alt="최신 릴리스"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ArcRelayProject/arcrelay?style=flat-square&color=16a34a" alt="AGPL-3.0-only"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a> · 한국어 · <a href="README.de.md">Deutsch</a> · <a href="README.fr.md">Français</a> · <a href="README.es.md">Español</a> · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center"><img src="assets/screenshots/quick-actions.png" alt="ArcRelay 빠른 동작" width="920"></p>

> 화면은 현재 ArcRelay 데스크톱 프런트엔드에서 촬영했습니다. 기기와 활동 내용은 개발용 예시 데이터입니다.

## 주요 기능

ArcRelay는 로컬 네트워크에서 기기를 찾고 인증된 암호화 연결을 만듭니다. 데스크톱 협업에는 클라우드 계정이 필요하지 않으며, 사용자가 허용한 기능만 상대 기기에 공개합니다.

| | 기능 | 용도 |
| --- | --- | --- |
| ⚡ | 빠른 동작 | 앱, 폴더, 스크립트와 자주 쓰는 작업을 검색하고 실행합니다. |
| 🔁 | 로컬 자동화 | 트리거, 조건, 확인, 순차 단계를 조합하고 실행 기록을 보존합니다. |
| 📋 | 클립보드 작업 공간 | 텍스트와 이미지를 검색하고 다시 사용합니다. |
| 📦 | 주변 전송 | 승인한 기기로 파일을 직접 암호화하여 보냅니다. |
| 📁 | 원격 파일 | 허용된 공유 폴더를 탐색하고 업로드하거나 다운로드합니다. |
| ⌨️ | 화면 간 입력 | 디스플레이를 배치하고 마우스와 키보드를 여러 컴퓨터 사이에서 이동합니다. |
| 🖨️ | 프린터 공유 | 로컬 프린터를 공유하고 원격 인쇄 작업을 추적합니다. |
| 🛡️ | 프레젠테이션 보호 | 화면 공유 중 선택한 앱 창을 가립니다. |

## 제품 화면

<table><tr>
<td width="50%"><img src="assets/screenshots/nearby-transfer.png" alt="주변 파일 전송"></td>
<td width="50%"><img src="assets/screenshots/input-workspace.png" alt="화면 간 입력"></td>
</tr><tr>
<td align="center"><strong>주변 전송</strong><br>진행률, 무결성 확인, 수신 제어를 제공하는 직접 전송.</td>
<td align="center"><strong>화면 간 입력</strong><br>화면, 연결, 진단을 시각적으로 관리.</td>
</tr></table>

## 개인정보 보호와 보안

- QUIC 및 TLS 1.3으로 기기 연결을 보호합니다.
- 페어링은 기기 신원과 기능별 권한을 기록합니다.
- 업데이트 패키지는 Tauri updater 키로 서명하며 설치 전에 검증합니다.
- 공식 macOS 패키지는 Developer ID 서명과 Apple 공증을 거칩니다.
- 보안 문제는 [보안 정책](SECURITY.md)에 따라 비공개로 신고할 수 있습니다.

## 다운로드와 업데이트 채널

| 채널 | 대상 | 배포 방식 |
| --- | --- | --- |
| **Stable** | 일상 사용 | 관리자가 `vMAJOR.MINOR.PATCH` 태그를 만들면 배포합니다. |
| **Test** | 사전 테스트 | `main` CI가 성공할 때마다 자동 배포하며 완성되지 않은 변경이 포함될 수 있습니다. |

**[GitHub Releases](https://github.com/ArcRelayProject/arcrelay/releases)** 에서 다운로드하세요. 현재 공식 대상은 Apple Silicon용 macOS와 x86-64용 Windows입니다. 공식 모바일 앱은 별도로 배포되며 소스 코드는 이 저장소에 포함되지 않습니다.

**설정 → 일반 → 업데이트 채널**에서 Stable 또는 Test를 선택할 수 있습니다. 두 채널 모두 HTTPS 매니페스트와 앱에 내장된 동일한 공개 키를 사용합니다. Test 배포는 Stable 채널을 변경하지 않습니다.

## 소스에서 실행

Rust stable, Node.js 22, Tauri 2 플랫폼 개발 의존성이 필요합니다.

```bash
git clone https://github.com/ArcRelayProject/arcrelay.git
cd arcrelay
npm ci
npm run tauri -- dev
```

이 저장소에는 Tauri 호스트, Svelte UI, 데스크톱 통합, 릴리스 자동화가 들어 있습니다. 이슈와 범위가 명확한 Pull Request를 환영합니다. 제출하기 전에 [CONTRIBUTING.md](CONTRIBUTING.md)를 읽어 주세요.

## 라이선스와 상표

Copyright © 2026 Shenzhen Changning Technology Co., Ltd.

소스 코드는 [GNU AGPL v3.0 only](LICENSE)에 따라 제공됩니다. ArcRelay 이름, 로고와 브랜드 자산은 [TRADEMARKS.md](TRADEMARKS.md)를 확인하세요.
