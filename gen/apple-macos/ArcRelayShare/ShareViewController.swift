import AppKit
import Foundation

private let appGroup = "group.com.arcrelay.shared"
private let fileURLType = "public.file-url"
private let maximumFileCount = 256
private let maximumTotalBytes: UInt64 = 16 * 1024 * 1024 * 1024 * 1024

private struct PeerCache: Decodable {
    let peers: [SharePeer]
}

private struct SharePeer: Decodable {
    let id: String
    let name: String
    let platform: String
    let model: String
}

private struct NativeFile: Encodable {
    let path: String
}

private struct NativeRequest: Encodable {
    let version: Int
    let id: String
    let source: String
    let createdAtMs: Int64
    let targetPeerId: String?
    let files: [NativeFile]
}

final class ShareViewController: NSViewController {
    private let titleLabel = NSTextField(labelWithString: NSLocalizedString("发送到附近设备", comment: "Share title"))
    private let summaryLabel = NSTextField(labelWithString: NSLocalizedString("正在读取所选文件…", comment: "Loading files"))
    private let peerPicker = NSPopUpButton(frame: .zero, pullsDown: false)
    private let statusLabel = NSTextField(labelWithString: "")
    private let sendButton = NSButton(title: NSLocalizedString("继续", comment: "Continue share"), target: nil, action: nil)
    private let cancelButton = NSButton(title: NSLocalizedString("取消", comment: "Cancel share"), target: nil, action: nil)
    private var peers: [SharePeer] = []
    private var providers: [NSItemProvider] = []

    override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 440, height: 220))
        titleLabel.font = NSFont.systemFont(ofSize: 18, weight: .semibold)
        summaryLabel.textColor = .secondaryLabelColor
        statusLabel.textColor = .secondaryLabelColor
        statusLabel.lineBreakMode = .byWordWrapping
        statusLabel.maximumNumberOfLines = 2
        [titleLabel, summaryLabel, peerPicker, statusLabel, sendButton, cancelButton].forEach {
            $0.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview($0)
        }
        sendButton.target = self
        sendButton.action = #selector(send)
        sendButton.keyEquivalent = "\r"
        cancelButton.target = self
        cancelButton.action = #selector(cancel)
        cancelButton.keyEquivalent = "\u{1b}"
        NSLayoutConstraint.activate([
            titleLabel.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 24),
            titleLabel.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -24),
            titleLabel.topAnchor.constraint(equalTo: view.topAnchor, constant: 22),
            summaryLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            summaryLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            summaryLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 8),
            peerPicker.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            peerPicker.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            peerPicker.topAnchor.constraint(equalTo: summaryLabel.bottomAnchor, constant: 18),
            statusLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            statusLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            statusLabel.topAnchor.constraint(equalTo: peerPicker.bottomAnchor, constant: 10),
            sendButton.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            sendButton.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -20),
            cancelButton.trailingAnchor.constraint(equalTo: sendButton.leadingAnchor, constant: -10),
            cancelButton.centerYAnchor.constraint(equalTo: sendButton.centerYAnchor),
        ])
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        providers = (extensionContext?.inputItems as? [NSExtensionItem] ?? [])
            .flatMap { $0.attachments ?? [] }
            .filter { $0.hasItemConformingToTypeIdentifier(fileURLType) }
        summaryLabel.stringValue = String(format: NSLocalizedString("已选择 %d 个文件", comment: "Selected file count"), providers.count)
        peers = loadPeers()
        peerPicker.addItem(withTitle: NSLocalizedString("在 ArcRelay 中选择设备", comment: "Choose in main app"))
        for peer in peers {
            let details = [peer.platform, peer.model].filter { !$0.isEmpty }.joined(separator: " · ")
            peerPicker.addItem(withTitle: details.isEmpty ? peer.name : "\(peer.name) — \(details)")
        }
        sendButton.isEnabled = !providers.isEmpty && providers.count <= maximumFileCount
        if providers.isEmpty {
            statusLabel.stringValue = NSLocalizedString("当前分享内容不包含可读取的文件。", comment: "No files")
        } else if providers.count > maximumFileCount {
            statusLabel.stringValue = NSLocalizedString("一次最多分享 256 个文件。", comment: "Too many files")
        }
    }

    @objc private func cancel() {
        extensionContext?.cancelRequest(withError: NSError(
            domain: "com.arcrelay.desktop.share",
            code: NSUserCancelledError,
            userInfo: [NSLocalizedDescriptionKey: NSLocalizedString("分享已取消", comment: "Cancelled")]
        ))
    }

    @objc private func send() {
        guard sendButton.isEnabled else { return }
        let selectedIndex = peerPicker.indexOfSelectedItem
        let targetPeerID = selectedIndex > 0 ? peers[selectedIndex - 1].id : nil
        sendButton.isEnabled = false
        cancelButton.isEnabled = false
        statusLabel.stringValue = NSLocalizedString("正在准备文件…", comment: "Preparing")
        stageFiles(targetPeerID: targetPeerID) { [weak self] result in
            DispatchQueue.main.async {
                guard let self else { return }
                switch result {
                case .success(let requestURL):
                    self.openArcRelay(requestURL: requestURL)
                    self.extensionContext?.completeRequest(returningItems: nil, completionHandler: nil)
                case .failure(let error):
                    self.statusLabel.stringValue = error.localizedDescription
                    self.sendButton.isEnabled = true
                    self.cancelButton.isEnabled = true
                }
            }
        }
    }

    private func stageFiles(targetPeerID: String?, completion: @escaping (Result<URL, Error>) -> Void) {
        guard let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup) else {
            completion(.failure(NSError(domain: "com.arcrelay.desktop.share", code: 1, userInfo: [NSLocalizedDescriptionKey: "ArcRelay App Group is unavailable."])))
            return
        }
        let requestID = UUID().uuidString.lowercased()
        let requestDirectory = container.appendingPathComponent("ShareInbox/\(requestID)", isDirectory: true)
        let filesDirectory = requestDirectory.appendingPathComponent("files", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: filesDirectory, withIntermediateDirectories: true)
        } catch {
            completion(.failure(error))
            return
        }

        let group = DispatchGroup()
        let lock = NSLock()
        var staged = Array<URL?>(repeating: nil, count: providers.count)
        var firstError: Error?
        var totalBytes: UInt64 = 0
        var reservedDestinations = Set<String>()
        let expectedFileCount = providers.count
        for (index, provider) in providers.enumerated() {
            group.enter()
            provider.loadFileRepresentation(forTypeIdentifier: fileURLType) { url, error in
                defer { group.leave() }
                guard let source = url else {
                    lock.lock(); firstError = firstError ?? error ?? NSError(domain: "com.arcrelay.desktop.share", code: 2, userInfo: [NSLocalizedDescriptionKey: "A shared file could not be read."]); lock.unlock()
                    return
                }
                do {
                    let values = try source.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
                    guard values.isRegularFile == true else {
                        throw NSError(domain: "com.arcrelay.desktop.share", code: 3, userInfo: [NSLocalizedDescriptionKey: "ArcRelay currently accepts regular files only."])
                    }
                    let fileBytes = UInt64(max(values.fileSize ?? 0, 0))
                    lock.lock()
                    guard fileBytes <= maximumTotalBytes - min(totalBytes, maximumTotalBytes) else {
                        firstError = firstError ?? NSError(domain: "com.arcrelay.desktop.share", code: 5, userInfo: [NSLocalizedDescriptionKey: "The selected files exceed the transfer size limit."])
                        lock.unlock()
                        return
                    }
                    totalBytes += fileBytes
                    let destination = Self.uniqueDestination(in: filesDirectory, name: source.lastPathComponent, reserved: &reservedDestinations)
                    lock.unlock()
                    try FileManager.default.copyItem(at: source, to: destination)
                    lock.lock(); staged[index] = destination; lock.unlock()
                } catch {
                    lock.lock(); firstError = firstError ?? error; lock.unlock()
                }
            }
        }
        group.notify(queue: .global(qos: .userInitiated)) {
            if let error = firstError {
                try? FileManager.default.removeItem(at: requestDirectory)
                completion(.failure(error))
                return
            }
            let urls = staged.compactMap { $0 }
            guard urls.count == expectedFileCount else {
                try? FileManager.default.removeItem(at: requestDirectory)
                completion(.failure(NSError(domain: "com.arcrelay.desktop.share", code: 4, userInfo: [NSLocalizedDescriptionKey: "Some shared files were unavailable."])))
                return
            }
            let request = NativeRequest(
                version: 1,
                id: requestID,
                source: "macosShareExtension",
                createdAtMs: Int64(Date().timeIntervalSince1970 * 1000),
                targetPeerId: targetPeerID,
                files: urls.map { NativeFile(path: $0.path) }
            )
            do {
                let data = try JSONEncoder().encode(request)
                let requestURL = requestDirectory.appendingPathComponent("request.json")
                try data.write(to: requestURL, options: .atomic)
                completion(.success(requestURL))
            } catch {
                try? FileManager.default.removeItem(at: requestDirectory)
                completion(.failure(error))
            }
        }
    }

    private func loadPeers() -> [SharePeer] {
        guard let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup) else { return [] }
        let cacheURL = container.appendingPathComponent("ShareInbox/peers.json")
        guard let data = try? Data(contentsOf: cacheURL), let cache = try? JSONDecoder().decode(PeerCache.self, from: data) else { return [] }
        return cache.peers
    }

    private func openArcRelay(requestURL: URL) {
        if #available(macOS 10.15, *), let application = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.arcrelay.desktop") {
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.arguments = ["--arcrelay-share-request", requestURL.path]
            NSWorkspace.shared.openApplication(at: application, configuration: configuration) { _, _ in }
        } else if let path = NSWorkspace.shared.absolutePathForApplication(withBundleIdentifier: "com.arcrelay.desktop") {
            _ = NSWorkspace.shared.launchApplication(path)
        }
    }

    private static func uniqueDestination(in directory: URL, name: String, reserved: inout Set<String>) -> URL {
        let clean = name.isEmpty ? "Shared file" : name.replacingOccurrences(of: "/", with: "_")
        let original = directory.appendingPathComponent(clean)
        if !FileManager.default.fileExists(atPath: original.path) && reserved.insert(original.path).inserted {
            return original
        }
        let stem = original.deletingPathExtension().lastPathComponent
        let suffix = original.pathExtension
        for index in 1...9999 {
            let candidateName = suffix.isEmpty ? "\(stem) (\(index))" : "\(stem) (\(index)).\(suffix)"
            let candidate = directory.appendingPathComponent(candidateName)
            if !FileManager.default.fileExists(atPath: candidate.path) && reserved.insert(candidate.path).inserted {
                return candidate
            }
        }
        let fallback = directory.appendingPathComponent(UUID().uuidString)
        reserved.insert(fallback.path)
        return fallback
    }
}
