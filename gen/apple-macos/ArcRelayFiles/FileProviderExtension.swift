import Foundation
import FileProvider
import UniformTypeIdentifiers

struct RemoteItem: Codable {
    var id: String
    var parentId: String
    var name: String
    var path: String
    var folder: Bool
    var size: UInt64
    var modifiedAtMs: Int64
    var revision: String
    var writable: Bool
}

final class ProviderItem: NSObject, NSFileProviderItem {
    let remote: RemoteItem
    init(_ remote: RemoteItem) { self.remote = remote }
    static func identifier(_ id: String) -> NSFileProviderItemIdentifier {
        id == "root" ? .rootContainer : NSFileProviderItemIdentifier(id)
    }
    static func key(_ id: NSFileProviderItemIdentifier) -> String {
        id == .rootContainer ? "root" : id.rawValue
    }
    var itemIdentifier: NSFileProviderItemIdentifier { Self.identifier(remote.id) }
    var parentItemIdentifier: NSFileProviderItemIdentifier { Self.identifier(remote.parentId) }
    var filename: String { remote.name }
    var contentType: UTType {
        remote.folder ? .folder : UTType(filenameExtension: (remote.name as NSString).pathExtension) ?? .data
    }
    var documentSize: NSNumber? { NSNumber(value: remote.size) }
    var contentModificationDate: Date? { Date(timeIntervalSince1970: Double(remote.modifiedAtMs) / 1000) }
    var itemVersion: NSFileProviderItemVersion {
        NSFileProviderItemVersion(contentVersion: Data(remote.revision.utf8), metadataVersion: Data(remote.revision.utf8))
    }
    var capabilities: NSFileProviderItemCapabilities {
        var result: NSFileProviderItemCapabilities = remote.folder ? [.allowsContentEnumerating, .allowsReading] : [.allowsReading]
        if remote.writable {
            if remote.folder { result.insert(.allowsAddingSubItems) } else { result.insert(.allowsWriting) }
            if remote.id != "root" { result.formUnion([.allowsRenaming, .allowsReparenting, .allowsDeleting]) }
        }
        return result
    }
}

private struct BridgeConnection: Decodable { let port: Int; let token: String }
private struct BridgeFailure: Decodable { let code: String; let message: String }
private struct Listing: Decodable { let items: [RemoteItem]; let anchor: UInt64? }
private struct RemoteChange: Decodable { let sequence: UInt64; let item: RemoteItem; let deleted: Bool }
private struct Changes: Decodable { let changes: [RemoteChange]; let anchor: UInt64 }

private func providerError(_ error: Error) -> Error {
    if error is CancellationError { return NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError) }
    let ns = error as NSError
    if ns.domain == NSFileProviderErrorDomain || ns.domain == NSCocoaErrorDomain { return error }
    return NSError(domain: NSFileProviderErrorDomain, code: NSFileProviderError.serverUnreachable.rawValue,
        userInfo: [NSLocalizedDescriptionKey: "Open ArcRelay and reconnect the other device. Downloaded files and local edits are retained.", NSUnderlyingErrorKey: error])
}

private final class Bridge {
    let domain: String
    private let session = URLSession(configuration: .ephemeral)
    init(domain: String) { self.domain = domain }
    func invalidate() { session.invalidateAndCancel() }

    func request(_ path: String, query: [String: String] = [:]) throws -> URLRequest {
        guard let group = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.com.arcrelay.shared") else {
            throw NSFileProviderError(.serverUnreachable)
        }
        let connection = try JSONDecoder().decode(BridgeConnection.self,
            from: Data(contentsOf: group.appendingPathComponent("FileProvider/connection.json")))
        guard (1...65535).contains(connection.port), connection.token.count == 64 else { throw NSFileProviderError(.notAuthenticated) }
        var url = URLComponents()
        url.scheme = "http"
        url.host = "127.0.0.1"
        url.port = connection.port
        url.path = path
        url.queryItems = query.map { URLQueryItem(name: $0.key, value: $0.value) }
        var request = URLRequest(url: url.url!)
        request.timeoutInterval = 300
        request.setValue("Bearer \(connection.token)", forHTTPHeaderField: "Authorization")
        return request
    }

    func check(_ data: Data, _ response: URLResponse) throws {
        guard let response = response as? HTTPURLResponse else { throw NSFileProviderError(.serverUnreachable) }
        guard (200..<300).contains(response.statusCode) else {
            let failure = try? JSONDecoder().decode(BridgeFailure.self, from: data)
            let code: NSFileProviderError.Code
            switch failure?.code {
            case "notFound": code = .noSuchItem
            case "anchorExpired": code = .syncAnchorExpired
            case "quota": code = .insufficientQuota
            case "conflict": code = .cannotSynchronize
            case "forbidden":
                throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteNoPermissionError,
                    userInfo: [NSLocalizedDescriptionKey: failure?.message ?? "Sharing permission was removed."])
            default: code = .serverUnreachable
            }
            throw NSError(domain: NSFileProviderErrorDomain, code: code.rawValue,
                userInfo: [NSLocalizedDescriptionKey: failure?.message ?? "The other device is unavailable."])
        }
    }

    func operation<T: Decodable>(_ action: String, fields: [String: Any] = [:], as: T.Type) async throws -> T {
        var request = try request("/v1/operation")
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        var body = fields
        body["domain"] = domain
        body["action"] = action
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await session.data(for: request)
        try check(data, response)
        return try JSONDecoder().decode(T.self, from: data)
    }

    func save(id: String?, parent: String, name: String, revision: String, contents: URL) async throws -> RemoteItem {
        var query = ["domain": domain, "parent": parent, "name": name, "revision": revision]
        query["item"] = id
        var request = try request("/v1/content", query: query)
        request.httpMethod = "PUT"
        request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
        let (data, response) = try await session.upload(for: request, fromFile: contents)
        try check(data, response)
        return try JSONDecoder().decode(RemoteItem.self, from: data)
    }

    func download(id: String, revision: String?) async throws -> (URL, RemoteItem) {
        let request = try request("/v1/content", query: ["domain": domain, "item": id, "revision": revision ?? ""])
        let (url, response) = try await session.download(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            let errorBody = (try? Data(contentsOf: url)) ?? Data()
            try? FileManager.default.removeItem(at: url)
            try check(errorBody, response)
            throw NSFileProviderError(.serverUnreachable)
        }
        guard let encoded = http.value(forHTTPHeaderField: "X-ArcRelay-Item"), let data = Data(base64Encoded: encoded) else {
            throw NSFileProviderError(.serverUnreachable)
        }
        return (url, try JSONDecoder().decode(RemoteItem.self, from: data))
    }
}

final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let domain: NSFileProviderDomain
    private let bridge: Bridge
    required init(domain: NSFileProviderDomain) {
        self.domain = domain
        self.bridge = Bridge(domain: domain.identifier.rawValue)
        super.init()
    }
    func invalidate() { bridge.invalidate() }

    private func run<T>(_ work: @escaping () async throws -> T, completion: @escaping (T?, Error?) -> Void) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        let task = Task {
            do {
                let value = try await work()
                try Task.checkCancellation()
                progress.completedUnitCount = 1
                completion(value, nil)
            } catch { completion(nil, providerError(error)) }
        }
        progress.cancellationHandler = { task.cancel() }
        return progress
    }

    func item(for identifier: NSFileProviderItemIdentifier, request: NSFileProviderRequest,
              completionHandler: @escaping (NSFileProviderItem?, Error?) -> Void) -> Progress {
        run({ ProviderItem(try await self.bridge.operation("stat", fields: ["item": ProviderItem.key(identifier)], as: RemoteItem.self)) }, completion: completionHandler)
    }

    func fetchContents(for itemIdentifier: NSFileProviderItemIdentifier, version requestedVersion: NSFileProviderItemVersion?,
                       request: NSFileProviderRequest, completionHandler: @escaping (URL?, NSFileProviderItem?, Error?) -> Void) -> Progress {
        run({ () async throws -> (URL, ProviderItem) in
            let (download, item) = try await self.bridge.download(id: ProviderItem.key(itemIdentifier), revision: requestedVersion.flatMap { String(data: $0.contentVersion, encoding: .utf8) })
            guard let manager = NSFileProviderManager(for: self.domain) else { throw NSFileProviderError(.serverUnreachable) }
            let destination = try manager.temporaryDirectoryURL().appendingPathComponent(UUID().uuidString)
            try FileManager.default.moveItem(at: download, to: destination)
            return (destination, ProviderItem(item))
        }, completion: { value, error in completionHandler(value?.0, value?.1, error) })
    }

    func createItem(basedOn itemTemplate: NSFileProviderItem, fields: NSFileProviderItemFields, contents url: URL?,
                    options: NSFileProviderCreateItemOptions, request: NSFileProviderRequest,
                    completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void) -> Progress {
        run({
            let parent = ProviderItem.key(itemTemplate.parentItemIdentifier)
            let item: RemoteItem
            if itemTemplate.contentType == .folder {
                item = try await self.bridge.operation("mkdir", fields: ["parent": parent, "name": itemTemplate.filename], as: RemoteItem.self)
            } else {
                if let url {
                    item = try await self.bridge.save(id: nil, parent: parent, name: itemTemplate.filename, revision: "", contents: url)
                } else {
                    guard let manager = NSFileProviderManager(for: self.domain) else { throw NSFileProviderError(.serverUnreachable) }
                    let empty = try manager.temporaryDirectoryURL().appendingPathComponent(UUID().uuidString)
                    try Data().write(to: empty, options: .atomic)
                    defer { try? FileManager.default.removeItem(at: empty) }
                    item = try await self.bridge.save(id: nil, parent: parent, name: itemTemplate.filename, revision: "", contents: empty)
                }
            }
            return ProviderItem(item)
        }, completion: { item, error in completionHandler(item, [], false, error) })
    }

    func modifyItem(_ item: NSFileProviderItem, baseVersion version: NSFileProviderItemVersion, changedFields: NSFileProviderItemFields,
                    contents newContents: URL?, options: NSFileProviderModifyItemOptions, request: NSFileProviderRequest,
                    completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void) -> Progress {
        run({ () async throws -> (ProviderItem, NSFileProviderItemFields) in
            let id = ProviderItem.key(item.itemIdentifier)
            let parent = ProviderItem.key(item.parentItemIdentifier)
            var revision = String(data: version.contentVersion, encoding: .utf8) ?? ""
            var current: RemoteItem
            if changedFields.contains(.contents) {
                guard let newContents else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadUnknownError) }
                current = try await self.bridge.save(id: id, parent: parent, name: item.filename, revision: revision, contents: newContents)
                revision = current.revision
            } else {
                current = try await self.bridge.operation("stat", fields: ["item": id], as: RemoteItem.self)
            }
            if changedFields.contains(.filename) || changedFields.contains(.parentItemIdentifier) {
                do {
                    current = try await self.bridge.operation("move", fields: ["item": id, "parent": parent, "name": item.filename, "revision": revision], as: RemoteItem.self)
                } catch {
                    // A content upload may already be committed. Report that
                    // progress so the retry uses its new version, not the old one.
                    if changedFields.contains(.contents) {
                        return (ProviderItem(current), changedFields.intersection([.filename, .parentItemIdentifier]))
                    }
                    throw error
                }
            }
            return (ProviderItem(current), [])
        }, completion: { updated, error in completionHandler(updated?.0, updated?.1 ?? changedFields, false, error) })
    }

    func deleteItem(identifier: NSFileProviderItemIdentifier, baseVersion version: NSFileProviderItemVersion, options: NSFileProviderDeleteItemOptions,
                    request: NSFileProviderRequest, completionHandler: @escaping (Error?) -> Void) -> Progress {
        run({
            let id = ProviderItem.key(identifier)
            let revision = String(data: version.contentVersion, encoding: .utf8) ?? ""
            let _: [String: String] = try await self.bridge.operation("delete", fields: ["item": id, "revision": revision, "recursive": options.contains(.recursive)], as: [String: String].self)
            return true
        }, completion: { _, error in completionHandler(error) })
    }

    func enumerator(for containerItemIdentifier: NSFileProviderItemIdentifier, request: NSFileProviderRequest) throws -> NSFileProviderEnumerator {
        if containerItemIdentifier == .trashContainer { throw NSError(domain: NSCocoaErrorDomain, code: NSFeatureUnsupportedError) }
        return ProviderEnumerator(bridge: bridge, container: containerItemIdentifier)
    }
}

private final class ProviderEnumerator: NSObject, NSFileProviderEnumerator {
    let bridge: Bridge
    let container: NSFileProviderItemIdentifier
    private let lock = NSLock()
    private var snapshot: [RemoteItem]?
    init(bridge: Bridge, container: NSFileProviderItemIdentifier) { self.bridge = bridge; self.container = container }
    func invalidate() { lock.lock(); snapshot = nil; lock.unlock() }
    private func saveSnapshot(_ value: [RemoteItem]) { lock.lock(); snapshot = value; lock.unlock() }
    private func getSnapshot() -> [RemoteItem]? { lock.lock(); defer { lock.unlock() }; return snapshot }

    func enumerateItems(for observer: NSFileProviderEnumerationObserver, startingAt page: NSFileProviderPage) {
        Task {
            do {
                let offset: Int
                if page.rawValue == NSFileProviderPage.initialPageSortedByName as Data || page.rawValue == NSFileProviderPage.initialPageSortedByDate as Data {
                    let listing: Listing = try await bridge.operation(container == .workingSet ? "workingSet" : "list",
                        fields: ["item": ProviderItem.key(container)], as: Listing.self)
                    saveSnapshot(listing.items)
                    offset = 0
                } else {
                    guard let text = String(data: page.rawValue, encoding: .utf8), let parsed = Int(text), parsed >= 0 else { throw NSFileProviderError(.pageExpired) }
                    offset = parsed
                }
                guard let items = getSnapshot(), offset <= items.count else { throw NSFileProviderError(.pageExpired) }
                let end = min(offset + 200, items.count)
                observer.didEnumerate(items[offset..<end].map(ProviderItem.init))
                observer.finishEnumerating(upTo: end == items.count ? nil : NSFileProviderPage(Data(String(end).utf8)))
            } catch { observer.finishEnumeratingWithError(providerError(error)) }
        }
    }

    func currentSyncAnchor(completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void) {
        Task {
            let listing = try? await bridge.operation("workingSet", as: Listing.self)
            completionHandler(listing?.anchor.map { NSFileProviderSyncAnchor(Data(String($0).utf8)) })
        }
    }

    func enumerateChanges(for observer: NSFileProviderChangeObserver, from syncAnchor: NSFileProviderSyncAnchor) {
        Task {
            do {
                guard let text = String(data: syncAnchor.rawValue, encoding: .utf8), let anchor = UInt64(text) else { throw NSFileProviderError(.syncAnchorExpired) }
                let response = try await bridge.operation("changes", fields: ["anchor": anchor], as: Changes.self)
                var latest: [String: RemoteChange] = [:]
                for change in response.changes where container == .workingSet || change.item.parentId == ProviderItem.key(container) || change.item.id == ProviderItem.key(container) {
                    latest[change.item.id] = change
                }
                let relevant = latest.values
                observer.didDeleteItems(withIdentifiers: relevant.filter(\.deleted).map { ProviderItem.identifier($0.item.id) })
                observer.didUpdate(relevant.filter { !$0.deleted }.map { ProviderItem($0.item) })
                observer.finishEnumeratingChanges(upTo: NSFileProviderSyncAnchor(Data(String(response.anchor).utf8)), moreComing: false)
            } catch { observer.finishEnumeratingWithError(providerError(error)) }
        }
    }
}
