use std::net::{IpAddr, SocketAddr};

use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::Response;
use axum::serve::IncomingStream;
use tokio::net::TcpListener;

use super::TRUNK_PORT;

#[derive(Clone)]
pub(super) struct Connection {
    local: Option<SocketAddr>,
    remote: SocketAddr,
}

impl Connected<IncomingStream<'_, TcpListener>> for Connection {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self {
            local: stream.io().local_addr().ok(),
            remote: *stream.remote_addr(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Authority {
    host: String,
    port: u16,
}

impl Authority {
    fn parse(value: &str) -> Option<Self> {
        let authority = value.parse::<axum::http::uri::Authority>().ok()?;

        if authority.as_str().contains('@') {
            return None;
        }

        let host = authority.host();
        let suffix = authority.as_str().strip_prefix(host)?;
        let host = host.trim_start_matches('[').trim_end_matches(']');

        let host = match host.parse::<IpAddr>() {
            Ok(ip) => ip.to_canonical().to_string(),
            Err(_) => host.to_ascii_lowercase(),
        };

        let port = match suffix {
            "" => 80,
            suffix => suffix.strip_prefix(':')?.parse().ok()?,
        };

        Some(Self { host, port })
    }

    fn origin(value: &str) -> Option<Self> {
        let uri = value.parse::<Uri>().ok()?;

        if uri.scheme_str() != Some("http")
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
            || value.ends_with('/')
        {
            return None;
        }

        Self::parse(uri.authority()?.as_str())
    }

    fn is_loopback(&self) -> bool {
        self.host == "localhost" || self.host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    /// Whether this authority names the socket the request arrived on.
    fn serves(&self, local: SocketAddr) -> bool {
        let ip = local.ip().to_canonical();
        self.port == local.port()
            && (self.host == ip.to_string() || (ip.is_loopback() && self.is_loopback()))
    }
}

/// Refuses requests whose `Host` does not name the socket they arrived on (DNS
/// rebinding), or whose `Origin` is a foreign page.
#[derive(Clone, Default)]
pub(super) struct Guard {
    dev: bool,
}

impl Guard {
    pub(super) fn new(dev: bool) -> Self {
        Self { dev }
    }

    fn allows(&self, headers: &HeaderMap, connection: &Connection) -> bool {
        let Some(local) = connection.local else {
            return false;
        };

        let Some(host) = single(headers, header::HOST).and_then(Authority::parse) else {
            return false;
        };

        let origin = if headers.contains_key(header::ORIGIN) {
            let Some(origin) = single(headers, header::ORIGIN).and_then(Authority::origin) else {
                return false;
            };

            Some(origin)
        } else {
            None
        };

        // `trunk serve` proxies /api and /ws from its loopback page; accept that
        // page only under --dev and only between loopback sockets.
        let dev_request = self.dev
            && local.ip().to_canonical().is_loopback()
            && connection.remote.ip().to_canonical().is_loopback()
            && origin
                .as_ref()
                .is_some_and(|o| o.port == TRUNK_PORT && o.is_loopback());

        // Trunk's WebSocket proxy omits the backend port from Host.
        let proxy_host = dev_request
            && (origin.as_ref() == Some(&host)
                || (host.port == 80
                    && Authority {
                        port: local.port(),
                        ..host.clone()
                    }
                    .serves(local)));

        if !host.serves(local) && !proxy_host {
            return false;
        }

        origin.is_none() || origin.as_ref() == Some(&host) || dev_request
    }
}

fn single(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

pub(super) async fn check(
    State(guard): State<Guard>,
    ConnectInfo(connection): ConnectInfo<Connection>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !guard.allows(request.headers(), &connection) {
        tracing::warn!(
            remote = %connection.remote,
            host = ?request.headers().get(header::HOST),
            origin = ?request.headers().get(header::ORIGIN),
            path = request.uri().path(),
            "refused request from foreign host or origin"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    request
        .extensions_mut()
        .insert(ConnectInfo(connection.remote));

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(local: &str) -> Connection {
        Connection {
            local: Some(local.parse().unwrap()),
            remote: "127.0.0.1:1234".parse().unwrap(),
        }
    }

    fn headers(host: &str, origin: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, host.parse().unwrap());

        if let Some(origin) = origin {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
        }

        headers
    }

    #[test]
    fn same_origin_and_loopback_hosts_are_accepted() {
        let guard = Guard::default();
        let local = connection("127.0.0.1:44114");

        for host in ["localhost:44114", "127.0.0.1:44114", "LOCALHOST:44114"] {
            assert!(guard.allows(&headers(host, None), &local), "{host}");
            let origin = format!("http://{host}");
            assert!(
                guard.allows(&headers(host, Some(&origin)), &local),
                "{host}"
            );
        }

        let v6 = connection("[::1]:44114");
        assert!(guard.allows(&headers("[::1]:44114", Some("http://[::1]:44114")), &v6));
        assert!(guard.allows(&headers("localhost:44114", None), &v6));

        let mapped = connection("[::ffff:127.0.0.1]:44114");
        assert!(guard.allows(&headers("127.0.0.1:44114", None), &mapped));

        let lan = connection("192.168.1.20:44114");
        assert!(guard.allows(
            &headers("192.168.1.20:44114", Some("http://192.168.1.20:44114")),
            &lan
        ));
        assert!(!guard.allows(&headers("localhost:44114", None), &lan));
    }

    #[test]
    fn foreign_origins_are_refused() {
        let guard = Guard::default();
        let local = connection("127.0.0.1:44114");

        for origin in [
            "http://evil.example",
            "http://evil.example:44114",
            "null",
            "https://localhost:44114",
            "http://localhost:44115",
            "http://localhost:8080",
            "http://localhost:44114/",
            "http://localhost:44114/path",
            "http://user@localhost:44114",
            "http://localhost:44114 http://evil.example",
        ] {
            assert!(
                !guard.allows(&headers("localhost:44114", Some(origin)), &local),
                "{origin}"
            );
        }

        let mut duplicate = headers("localhost:44114", Some("http://localhost:44114"));
        duplicate.append(header::ORIGIN, "http://evil.example".parse().unwrap());
        assert!(!guard.allows(&duplicate, &local));
    }

    #[test]
    fn foreign_hosts_are_refused() {
        let guard = Guard::default();
        let local = connection("127.0.0.1:44114");

        for host in [
            "evil.example:44114",
            "evil.example",
            "localhost:44115",
            "localhost",
            "user@localhost:44114",
            "localhost:99999",
        ] {
            assert!(!guard.allows(&headers(host, None), &local), "{host}");
        }

        let mut forged = headers("evil.example:44114", Some("http://evil.example:44114"));
        forged.insert("x-forwarded-host", "localhost:44114".parse().unwrap());
        assert!(!guard.allows(&forged, &local));
        assert!(!guard.allows(&HeaderMap::new(), &local));
    }

    #[test]
    fn trunk_origin_only_under_dev() {
        let local = connection("127.0.0.1:44614");

        for (host, origin) in [
            ("127.0.0.1:44614", "http://127.0.0.1:8080"),
            ("127.0.0.1", "http://127.0.0.1:8080"),
            ("localhost:8080", "http://localhost:8080"),
            ("localhost", "http://localhost:8080"),
        ] {
            let request = headers(host, Some(origin));
            assert!(
                !Guard::default().allows(&request, &local),
                "{host} {origin}"
            );
            assert!(Guard::new(true).allows(&request, &local), "{host} {origin}");
        }

        let guard = Guard::new(true);
        assert!(!guard.allows(&headers("127.0.0.1", Some("http://evil.example")), &local));
        assert!(!guard.allows(&headers("127.0.0.1", None), &local));
        assert!(!guard.allows(
            &headers("evil.example", Some("http://127.0.0.1:8080")),
            &local
        ));
        assert!(!guard.allows(
            &headers("evil.example:8080", Some("http://evil.example:8080")),
            &local
        ));

        let remote = Connection {
            remote: "192.168.1.2:1234".parse().unwrap(),
            ..local
        };
        assert!(!guard.allows(
            &headers("127.0.0.1:44614", Some("http://127.0.0.1:8080")),
            &remote
        ));
    }

    async fn request(address: SocketAddr, path: &str, host: &str, origin: Option<&str>) -> String {
        request_with(address, path, host, origin, None).await
    }

    async fn request_with(
        address: SocketAddr,
        path: &str,
        host: &str,
        origin: Option<&str>,
        cookie: Option<&str>,
    ) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let mut request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n");

        if path == "/ws" {
            request.push_str("Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n");
        } else {
            request.push_str("Connection: close\r\n");
        }

        if let Some(origin) = origin {
            request.push_str(&format!("Origin: {origin}\r\n"));
        }

        if let Some(cookie) = cookie {
            request.push_str(&format!("Cookie: {cookie}\r\n"));
        }

        request.push_str("\r\n");
        socket.write_all(request.as_bytes()).await.unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut response = Vec::new();

            while !response.ends_with(b"\r\n\r\n") {
                response.push(socket.read_u8().await.unwrap());
            }

            if path != "/ws" {
                socket.read_to_end(&mut response).await.unwrap();
            }

            String::from_utf8(response).unwrap()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn routes_refuse_foreign_requests() {
        let paths = crate::Paths::new(std::path::Path::new("/nonexistent"));
        let db = crate::Database::open(&paths, true).unwrap();
        let backend = crate::Backend::new(db, paths).await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(super::super::setup(listener, backend, false, None).unwrap());

        let host = address.to_string();
        let own = format!("http://{host}");
        let image = "/api/image/0/0";

        for path in ["/ws", image] {
            let foreign = request(address, path, &host, Some("http://evil.example")).await;
            assert!(foreign.starts_with("HTTP/1.1 403"), "{path}: {foreign}");
            let rebound = request(address, path, "evil.example", Some("http://evil.example")).await;
            assert!(rebound.starts_with("HTTP/1.1 403"), "{path}: {rebound}");
            let rebound = request(address, path, "evil.example", None).await;
            assert!(rebound.starts_with("HTTP/1.1 403"), "{path}: {rebound}");
            let trunk = request(address, path, &host, Some("http://127.0.0.1:8080")).await;
            assert!(trunk.starts_with("HTTP/1.1 403"), "{path}: {trunk}");

            for origin in [Some(own.as_str()), None] {
                let response = request(address, path, &host, origin).await;
                assert!(!response.starts_with("HTTP/1.1 403"), "{path}: {response}");
                assert!(
                    !response
                        .to_ascii_lowercase()
                        .contains("access-control-allow-origin"),
                    "{path}: {response}"
                );
            }
        }

        let upgrade = request(
            address,
            "/ws",
            &format!("localhost:{}", address.port()),
            Some(&own),
        )
        .await;
        assert!(upgrade.starts_with("HTTP/1.1 403"), "{upgrade}");
        let local = format!("localhost:{}", address.port());
        let upgrade = request(address, "/ws", &local, Some(&format!("http://{local}"))).await;
        assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");

        let access = request(address, "/api/access", &host, None).await;
        assert!(access.ends_with("\r\n\r\nopen"), "{access}");

        server.abort();
    }

    #[tokio::test]
    async fn sign_in_guards_routes() {
        use auth::{Auth, NewUser, Role};

        let paths = crate::Paths::new(std::path::Path::new("/nonexistent"));
        let db = crate::Database::open(&paths, true).unwrap();
        let store = crate::auth_store::AuthStore::new(db.clone());
        let backend = crate::Backend::new(db, paths).await.unwrap();
        let auth = Auth::builder_from_store(store).await.unwrap().build();

        let new = NewUser {
            login: Some(String::from("admin")),
            email: None,
            role: Role::Admin,
        };

        let user = auth
            .test_user(&new, "a long enough password")
            .await
            .unwrap();
        let session = auth.test_session(&user).await.unwrap();
        let cookie = auth.cookie_header(&session.id);
        let cookie = cookie.to_str().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server =
            tokio::spawn(super::super::setup(listener, backend, false, Some(auth)).unwrap());

        let host = address.to_string();
        let own = format!("http://{host}");

        let access = request(address, "/api/access", &host, None).await;
        assert!(access.ends_with("\r\n\r\nsign-in"), "{access}");

        let config = request(address, "/api/auth/config", &host, None).await;
        assert!(config.starts_with("HTTP/1.1 200"), "{config}");

        let foreign = request(
            address,
            "/api/auth/config",
            &host,
            Some("http://evil.example"),
        )
        .await;
        assert!(foreign.starts_with("HTTP/1.1 403"), "{foreign}");

        for path in ["/ws", "/api/image/0/0"] {
            let anonymous = request(address, path, &host, Some(&own)).await;
            assert!(anonymous.starts_with("HTTP/1.1 401"), "{path}: {anonymous}");
        }

        let image = request_with(address, "/api/image/0/0", &host, None, Some(cookie)).await;
        assert!(image.starts_with("HTTP/1.1 404"), "{image}");

        let upgrade = request_with(address, "/ws", &host, Some(&own), Some(cookie)).await;
        assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");

        server.abort();
    }

    #[test]
    fn only_loopback_binds_skip_sign_in() {
        for host in [
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
            "localhost",
            "LocalHost",
        ] {
            assert!(crate::is_loopback(host), "{host}");
        }

        for host in ["0.0.0.0", "::", "192.168.1.2", "example.com"] {
            assert!(!crate::is_loopback(host), "{host}");
        }
    }
}
