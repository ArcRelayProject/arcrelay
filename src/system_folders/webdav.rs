#![cfg_attr(not(target_os = "windows"), allow(dead_code))]
//! Loopback WebDAV adapter for the Windows WebClient redirector. Remote requests
//! still pass through the paired-device protocol; no LAN HTTP service is exposed.
use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::{Request as HttpRequest, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use server::{Bridge, ContentQuery};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Endpoint {
    pub port: u16,
    pub token: String,
}

#[derive(Clone)]
struct Dav {
    bridge: Bridge,
    endpoint: Endpoint,
}

pub(super) async fn start(service: Arc<SystemFolders>) -> Result<(), Error> {
    let path = service.root.join("webdav.json");
    let mut endpoint: Endpoint = match tokio::fs::read(&path).await {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| Error::unavailable(e.to_string()))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Endpoint {
            port: 0,
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        },
        Err(e) => return Err(e.into()),
    };
    if endpoint.token.len() != 64 || !endpoint.token.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::unavailable("invalid WebDAV endpoint identity"));
    }
    // Reuse the port so Explorer's persistent locations survive app restarts.
    // Fail closed if it was taken; never silently point a saved location elsewhere.
    let listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, endpoint.port)).await?;
    endpoint.port = listener.local_addr()?.port();
    crate::infrastructure::durable_file::replace_private(
        &path,
        &serde_json::to_vec(&endpoint).map_err(std::io::Error::other)?,
    )?;
    let routes = Router::new().fallback(any(handle)).with_state(Dav {
        bridge: Bridge::for_webdav(service.clone()),
        endpoint,
    });
    let stopped = service.stopped.clone();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, routes)
            .with_graceful_shutdown(stopped.cancelled_owned())
            .await
        {
            tracing::error!(%error, "Windows system folder server stopped");
        }
    });
    Ok(())
}

fn allowed(headers: &HeaderMap, endpoint: &Endpoint) -> bool {
    let authority = format!("127.0.0.1:{}", endpoint.port);
    headers.get("host").and_then(|h| h.to_str().ok()) == Some(authority.as_str())
        && !headers.contains_key("origin")
        && !headers.contains_key("sec-fetch-site")
}

fn invalid(message: &str) -> Error {
    Error {
        code: "invalid".into(),
        message: message.into(),
    }
}

fn decode(segment: &str) -> Result<String, Error> {
    let mut bytes = Vec::new();
    let mut source = segment.bytes();
    while let Some(byte) = source.next() {
        if byte == b'%' {
            let hi = source.next().and_then(|c| (c as char).to_digit(16));
            let lo = source.next().and_then(|c| (c as char).to_digit(16));
            bytes.push(match (hi, lo) {
                (Some(a), Some(b)) => (a * 16 + b) as u8,
                _ => return Err(invalid("invalid URL escape")),
            });
        } else {
            bytes.push(byte);
        }
    }
    let name = String::from_utf8(bytes).map_err(|_| invalid("invalid UTF-8 path"))?;
    validate_name(&name)?;
    Ok(name)
}

fn encode(name: &str) -> String {
    name.bytes()
        .map(|c| {
            if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) {
                (c as char).to_string()
            } else {
                format!("%{c:02X}")
            }
        })
        .collect()
}

fn location(path: &str, endpoint: &Endpoint) -> Result<(String, Vec<String>), Error> {
    let mut parts = path
        .strip_prefix('/')
        .ok_or_else(|| invalid("absolute path required"))?
        .split('/');
    if parts.next() != Some(endpoint.token.as_str()) {
        return Err(Error {
            code: "forbidden".into(),
            message: "invalid folder capability".into(),
        });
    }
    let domain = parts
        .next()
        .ok_or_else(|| invalid("folder identity required"))?;
    uuid::Uuid::parse_str(domain).map_err(|_| invalid("invalid folder identity"))?;
    let mut segments: Vec<_> = parts.collect();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    Ok((
        domain.to_owned(),
        segments.into_iter().map(decode).collect::<Result<_, _>>()?,
    ))
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn empty_property(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        format!("<{name}/>")
    } else {
        format!("<P:{name} xmlns:P=\"{}\"/>", xml(namespace))
    }
}

fn etag(item: &Item) -> String {
    // Revisions are opaque; hex encoding keeps arbitrary values out of headers.
    format!(
        "\"{}\"",
        item.revision
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

impl Dav {
    async fn resolve(&self, domain: &str, segments: &[String]) -> Result<Item, Error> {
        let service = &self.bridge.service;
        let mut item = service.stat(domain, "root").await?;
        for name in segments {
            if !item.folder {
                return Err(Error::missing("parent is not a directory"));
            }
            item = service
                .enumerate(domain, &item.id)
                .await?
                .into_iter()
                .find(|child| child.name == *name)
                .ok_or_else(|| Error::missing("file no longer exists"))?;
        }
        Ok(item)
    }

    fn property_response(
        endpoint: &Endpoint,
        domain: &str,
        item: &Item,
        properties: &Properties,
    ) -> String {
        let mut href = format!("/{}/{}/", endpoint.token, domain);
        if !item.path.is_empty() {
            href.push_str(
                &item
                    .path
                    .split('/')
                    .map(encode)
                    .collect::<Vec<_>>()
                    .join("/"),
            );
            if item.folder {
                href.push('/');
            }
        }
        let modified = chrono::DateTime::from_timestamp_millis(item.modified_at_ms)
            .unwrap_or_default()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let values = [
            ("displayname", xml(&item.name)),
            (
                "resourcetype",
                if item.folder {
                    "<D:collection/>".into()
                } else {
                    String::new()
                },
            ),
            ("getcontentlength", item.size.to_string()),
            ("getlastmodified", modified),
            (
                "creationdate",
                chrono::DateTime::from_timestamp_millis(item.modified_at_ms)
                    .unwrap_or_default()
                    .to_rfc3339(),
            ),
            ("getetag", xml(&etag(item))),
            (
                "getcontenttype",
                if item.folder {
                    "httpd/unix-directory"
                } else {
                    "application/octet-stream"
                }
                .into(),
            ),
            ("supportedlock", String::new()),
            ("lockdiscovery", String::new()),
        ];
        let mut found = String::new();
        let mut missing = String::new();
        match properties {
            Properties::All | Properties::Names => {
                for (name, value) in &values {
                    found.push_str(&format!(
                        "<D:{name}>{}</D:{name}>",
                        if matches!(properties, Properties::Names) {
                            ""
                        } else {
                            value
                        }
                    ));
                }
            }
            Properties::Selected(names) => {
                for (ns, name) in names {
                    if let Some((_, value)) =
                        values.iter().find(|(key, _)| ns == "DAV:" && key == name)
                    {
                        found.push_str(&format!("<D:{name}>{value}</D:{name}>"));
                    } else {
                        missing.push_str(&empty_property(ns, name));
                    }
                }
            }
        }
        let mut result = format!("<D:response><D:href>{}</D:href>", xml(&href));
        for (props, status) in [(found, "200 OK"), (missing, "404 Not Found")] {
            if !props.is_empty() {
                result.push_str(&format!("<D:propstat><D:prop>{props}</D:prop><D:status>HTTP/1.1 {status}</D:status></D:propstat>"));
            }
        }
        result.push_str("</D:response>");
        result
    }
}

enum Properties {
    All,
    Names,
    Selected(Vec<(String, String)>),
}
fn properties(bytes: &[u8]) -> Result<Properties, Error> {
    if bytes.is_empty() {
        return Ok(Properties::All);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("invalid property request"))?;
    let doc = roxmltree::Document::parse(text).map_err(|_| invalid("invalid property XML"))?;
    let root = doc.root_element();
    if !root.has_tag_name(("DAV:", "propfind")) {
        return Err(invalid("expected DAV:propfind"));
    }
    for child in root.children().filter(|n| n.is_element()) {
        if child.has_tag_name(("DAV:", "allprop")) {
            return Ok(Properties::All);
        }
        if child.has_tag_name(("DAV:", "propname")) {
            return Ok(Properties::Names);
        }
        if child.has_tag_name(("DAV:", "prop")) {
            return Ok(Properties::Selected(
                child
                    .children()
                    .filter(|n| n.is_element())
                    .map(|n| {
                        (
                            n.tag_name().namespace().unwrap_or("").into(),
                            n.tag_name().name().into(),
                        )
                    })
                    .collect(),
            ));
        }
    }
    Err(invalid("unsupported property request"))
}

async fn ancestor(request: HttpRequest) -> Result<Response, Error> {
    if !matches!(request.method().as_str(), "PROPFIND" | "HEAD") {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    if request.method() == "HEAD" {
        return Ok(StatusCode::OK.into_response());
    }
    let path = request.uri().path().to_string();
    let bytes = to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| invalid("property request exceeds 64 KiB"))?;
    let props = properties(&bytes)?;
    let mut found = String::new();
    let mut missing = String::new();
    let names = match props {
        Properties::Selected(names) => names,
        _ => vec![("DAV:".into(), "resourcetype".into())],
    };
    for (ns, name) in names {
        if ns == "DAV:" && name == "resourcetype" {
            found.push_str("<D:resourcetype><D:collection/></D:resourcetype>");
        } else {
            missing.push_str(&empty_property(&ns, &name));
        }
    }
    let mut result = format!(
        "<D:multistatus xmlns:D=\"DAV:\"><D:response><D:href>{}</D:href>",
        xml(&path)
    );
    for (value, status) in [(found, "200 OK"), (missing, "404 Not Found")] {
        if !value.is_empty() {
            result.push_str(&format!("<D:propstat><D:prop>{value}</D:prop><D:status>HTTP/1.1 {status}</D:status></D:propstat>"));
        }
    }
    result.push_str("</D:response></D:multistatus>");
    Ok((
        StatusCode::MULTI_STATUS,
        [("content-type", "application/xml; charset=utf-8")],
        result,
    )
        .into_response())
}

const METHODS: &str = "OPTIONS, PROPFIND, GET, HEAD, PUT, MKCOL, MOVE, DELETE";
async fn handle(State(dav): State<Dav>, request: HttpRequest) -> Response {
    if !allowed(request.headers(), &dav.endpoint) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // The redirector probes server ancestors before opening a network location.
    // OPTIONS discloses only protocol support, never shares or metadata.
    let mut response = if request.method() == "OPTIONS" {
        StatusCode::NO_CONTENT.into_response()
    } else {
        match dispatch(&dav, request).await {
            Ok(response) => response,
            Err(error) => error.into_response(),
        }
    };
    response.headers_mut().insert("dav", "1".parse().unwrap());
    response
        .headers_mut()
        .insert("allow", METHODS.parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

fn condition(headers: &HeaderMap, item: Option<&Item>) -> bool {
    if let Some(value) = headers.get("if-match") {
        let Ok(value) = value.to_str() else {
            return false;
        };
        if !item.is_some_and(|item| {
            value == "*" || value.split(',').any(|part| part.trim() == etag(item))
        }) {
            return false;
        }
    }
    if let Some(value) = headers.get("if-none-match") {
        let Ok(value) = value.to_str() else {
            return false;
        };
        if item.is_some_and(|item| {
            value == "*" || value.split(',').any(|part| part.trim() == etag(item))
        }) {
            return false;
        }
    }
    // We do not advertise locks and must not ignore a lock/ETag condition.
    !headers.contains_key("if")
}

async fn dispatch(dav: &Dav, request: HttpRequest) -> Result<Response, Error> {
    let path = request.uri().path();
    // WebClient probes ancestors of DavWWWRoot. These synthetic collections
    // reveal no domain names, identities or contents, even at Depth: 1.
    if path == "/" || path.trim_end_matches('/') == format!("/{}", dav.endpoint.token) {
        return ancestor(request).await;
    }
    let (domain, segments) = location(request.uri().path(), &dav.endpoint)?;
    let service = &dav.bridge.service;
    service.domain(&domain).await?;
    let (parts, body) = request.into_parts();
    let method = parts.method.as_str();
    if !matches!(
        method,
        "PROPFIND" | "GET" | "HEAD" | "PUT" | "MKCOL" | "MOVE" | "DELETE"
    ) {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    if method == "PUT" || method == "MKCOL" {
        let (name, parents) = segments
            .split_last()
            .ok_or_else(|| Error::conflict("cannot replace the shared root"))?;
        let parent = dav.resolve(&domain, parents).await?;
        if !parent.folder {
            return Err(Error::conflict("parent is not a directory"));
        }
        let existing = service
            .enumerate(&domain, &parent.id)
            .await?
            .into_iter()
            .find(|item| item.name == *name);
        if !condition(&parts.headers, existing.as_ref()) {
            return Ok(StatusCode::PRECONDITION_FAILED.into_response());
        }
        if method == "MKCOL" {
            if existing.is_some() {
                return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
            }
            if !to_bytes(body, 1)
                .await
                .map_err(|_| invalid("MKCOL body is unsupported"))?
                .is_empty()
            {
                return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
            }
            service.mkdir(&domain, &parent.id, name).await?;
        } else {
            if existing.as_ref().is_some_and(|item| item.folder) {
                return Err(Error::conflict("cannot replace a directory"));
            }
            let query = ContentQuery {
                domain,
                item: existing.as_ref().map(|item| item.id.clone()),
                parent: parent.id,
                name: name.clone(),
                revision: existing
                    .as_ref()
                    .map(|item| item.revision.clone())
                    .unwrap_or_default(),
            };
            let axum::Json(item) =
                server::upload(State(dav.bridge.clone()), axum::extract::Query(query), body)
                    .await?;
            let mut response = if existing.is_some() {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::CREATED
            }
            .into_response();
            response
                .headers_mut()
                .insert("etag", etag(&item).parse().unwrap());
            return Ok(response);
        }
        return Ok(StatusCode::CREATED.into_response());
    }
    let item = dav.resolve(&domain, &segments).await?;
    if !condition(&parts.headers, Some(&item)) {
        return Ok(StatusCode::PRECONDITION_FAILED.into_response());
    }
    match method {
        "PROPFIND" => {
            let depth = parts
                .headers
                .get("depth")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("infinity");
            if !matches!(depth, "0" | "1") {
                return Ok((
                    StatusCode::FORBIDDEN,
                    [("content-type", "application/xml; charset=utf-8")],
                    "<D:error xmlns:D=\"DAV:\"><D:propfind-finite-depth/></D:error>",
                )
                    .into_response());
            }
            let bytes = to_bytes(body, 64 * 1024)
                .await
                .map_err(|_| invalid("property request exceeds 64 KiB"))?;
            let properties = properties(&bytes)?;
            let mut result = String::from(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><D:multistatus xmlns:D=\"DAV:\">",
            );
            result.push_str(&Dav::property_response(
                &dav.endpoint,
                &domain,
                &item,
                &properties,
            ));
            if depth == "1" && item.folder {
                for child in service.enumerate(&domain, &item.id).await? {
                    result.push_str(&Dav::property_response(
                        &dav.endpoint,
                        &domain,
                        &child,
                        &properties,
                    ));
                }
            }
            result.push_str("</D:multistatus>");
            Ok((
                StatusCode::MULTI_STATUS,
                [("content-type", "application/xml; charset=utf-8")],
                result,
            )
                .into_response())
        }
        "GET" | "HEAD" => {
            if item.folder {
                return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
            }
            let mut response = if method == "HEAD" {
                Response::builder()
                    .header("content-length", item.size)
                    .body(Body::empty())
                    .unwrap()
            } else {
                server::content(
                    State(dav.bridge.clone()),
                    axum::extract::Query(ContentQuery {
                        domain,
                        item: Some(item.id.clone()),
                        parent: item.parent_id.clone(),
                        name: item.name.clone(),
                        revision: item.revision.clone(),
                    }),
                )
                .await?
            };
            response.headers_mut().remove("x-arcrelay-item");
            response
                .headers_mut()
                .insert("etag", etag(&item).parse().unwrap());
            // Range is deliberately ignored: HTTP permits a full 200 response.
            Ok(response)
        }
        "DELETE" => {
            let depth = parts
                .headers
                .get("depth")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("infinity");
            if depth != "infinity" {
                return Err(invalid("DELETE requires Depth: infinity"));
            }
            service
                .delete(&domain, &item.id, &item.revision, item.folder)
                .await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        "MOVE" => {
            let destination = parts
                .headers
                .get("destination")
                .and_then(|h| h.to_str().ok())
                .ok_or_else(|| invalid("destination required"))?;
            let uri: axum::http::Uri = destination
                .parse()
                .map_err(|_| invalid("invalid destination"))?;
            if uri
                .authority()
                .is_some_and(|a| a.as_str() != format!("127.0.0.1:{}", dav.endpoint.port))
                || uri.scheme_str().is_some_and(|s| s != "http")
                || uri.query().is_some()
            {
                return Err(invalid("destination must be on the same local server"));
            }
            let (other, target) = location(uri.path(), &dav.endpoint)?;
            if other != domain {
                return Err(Error::conflict("cross-share moves are unsupported"));
            }
            let (name, parents) = target
                .split_last()
                .ok_or_else(|| Error::conflict("cannot replace the shared root"))?;
            let parent = dav.resolve(&domain, parents).await?;
            // Never destroy an existing destination, even if Overwrite defaults to T.
            if service
                .enumerate(&domain, &parent.id)
                .await?
                .iter()
                .any(|i| i.name == *name)
            {
                return Ok(StatusCode::PRECONDITION_FAILED.into_response());
            }
            service
                .move_item(&domain, &item.id, &parent.id, name, &item.revision)
                .await?;
            Ok(StatusCode::CREATED.into_response())
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn endpoint() -> Endpoint {
        Endpoint {
            port: 1234,
            token: "secret".into(),
        }
    }
    const ID: &str = "12345678-1234-1234-1234-123456789abc";
    #[test]
    fn paths_require_capability_and_reject_traversal_and_encoded_separators() {
        for suffix in ["..", "%2e%2e", "%2fetc", "%5cfile", "%00", "a//b", "%zz"] {
            assert!(
                location(&format!("/secret/{ID}/{suffix}"), &endpoint()).is_err(),
                "{suffix}"
            );
        }
        assert!(location(&format!("/wrong/{ID}/"), &endpoint()).is_err());
        let name = "报告 & #1.txt";
        assert_eq!(
            location(&format!("/secret/{ID}/{}", encode(name)), &endpoint())
                .unwrap()
                .1,
            vec![name]
        );
    }
    #[test]
    fn only_local_non_browser_requests_are_accepted() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:1234".parse().unwrap());
        assert!(allowed(&headers, &endpoint()));
        headers.insert("sec-fetch-site", "none".parse().unwrap());
        assert!(!allowed(&headers, &endpoint()));
        headers.remove("sec-fetch-site");
        headers.insert("host", "evil.example:1234".parse().unwrap());
        assert!(!allowed(&headers, &endpoint()));
    }
    #[test]
    fn etag_preconditions_do_not_silently_overwrite() {
        let mut item = Index::root("share", true).item("root").unwrap();
        item.revision = "v1".into();
        let mut headers = HeaderMap::new();
        headers.insert("if-match", etag(&item).parse().unwrap());
        assert!(condition(&headers, Some(&item)));
        item.revision = "v2".into();
        assert!(!condition(&headers, Some(&item)));
        assert!(!condition(&headers, None));
        headers.clear();
        headers.insert("if-none-match", "*".parse().unwrap());
        assert!(condition(&headers, None));
        assert!(!condition(&headers, Some(&item)));
        headers.clear();
        headers.insert("if", "(<opaquelocktoken:unknown>)".parse().unwrap());
        assert!(!condition(&headers, Some(&item)));
    }
    #[test]
    fn parses_namespaced_property_requests_and_rejects_entities() {
        assert!(
            matches!(properties(b"<propfind xmlns='DAV:'><prop><getetag/><custom xmlns='urn:custom'/></prop></propfind>").unwrap(), Properties::Selected(p) if p == vec![("DAV:".into(), "getetag".into()), ("urn:custom".into(), "custom".into())])
        );
        assert!(properties(b"<!DOCTYPE x [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><propfind xmlns='DAV:'>&x;</propfind>").is_err());
    }
    #[test]
    fn metadata_is_valid_xml_and_reports_unknown_properties_separately() {
        let mut item = Index::root("报告 & <docs>", true).item("root").unwrap();
        item.path = "报告 & <docs>".into();
        let props = Properties::Selected(vec![
            ("DAV:".into(), "resourcetype".into()),
            ("urn:test".into(), "custom".into()),
            ("".into(), "bare".into()),
        ]);
        let response = Dav::property_response(&endpoint(), ID, &item, &props);
        let xml = format!("<D:multistatus xmlns:D=\"DAV:\">{response}</D:multistatus>");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let statuses: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name(("DAV:", "status")))
            .map(|n| n.text().unwrap())
            .collect();
        assert_eq!(statuses, vec!["HTTP/1.1 200 OK", "HTTP/1.1 404 Not Found"]);
        let href = doc
            .descendants()
            .find(|n| n.has_tag_name(("DAV:", "href")))
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(location(href, &endpoint()).unwrap().1, vec![item.path]);
    }

    #[tokio::test]
    async fn redirector_ancestor_probes_are_collections_without_share_disclosure() {
        for path in ["/", "/secret/"] {
            let request = HttpRequest::builder()
                .method("PROPFIND")
                .uri(path)
                .header("depth", "1")
                .body(Body::empty())
                .unwrap();
            let response = ancestor(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::MULTI_STATUS);
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            let doc = roxmltree::Document::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
            assert_eq!(
                doc.descendants()
                    .filter(|n| n.has_tag_name(("DAV:", "response")))
                    .count(),
                1
            );
            assert!(doc
                .descendants()
                .any(|n| n.has_tag_name(("DAV:", "collection"))));
        }
        let request = HttpRequest::builder()
            .method("PUT")
            .uri("/")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            ancestor(request).await.unwrap().status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}
