//! `java.net`: real HTTP(S), as a plugin has it on Paper. `URL.openConnection()` is an `HttpURLConnection`,
//! `HttpClient.send` a request and its response; a failure is the `IOException` Java throws (update checkers
//! and webhooks already catch it).
use crate::{R, V, exc, native};

/// `HttpClient`, and its builder.
pub struct JHttpClient;
/// `HttpRequest` and its builder: what is to be sent.
pub struct JHttpPart(pub std::sync::Mutex<Request>);
/// A body publisher (`BodyPublishers.ofString(text)`) or handler (`BodyHandlers.ofString()`).
pub struct JBodyPart(pub Option<Vec<u8>>, pub &'static str);
/// `HttpResponse`: the answer, the address asked and what the body is to be.
pub struct JHttpResponse(Answer, String, &'static str);
/// `HttpHeaders` of a response.
pub struct JResponseHeaders(Vec<(String, String)>);
/// `HttpURLConnection`.
pub struct JConnection(std::sync::Mutex<Connection>);

#[derive(Clone, Default)]
pub struct Request {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) timeout_ms: u64,
}

#[derive(Clone)]
pub struct Answer {
    pub(crate) status: i32,
    pub(crate) message: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

pub struct Connection {
    request: Request,
    out: Option<V>,
    answer: Option<Answer>,
}

/// Sends a request: the answer, whatever its status, or the `IOException` that stopped it.
pub(crate) fn send(request: &Request) -> Result<Answer, V> {
    use std::io::Read as _;
    let timeout = std::time::Duration::from_millis(if request.timeout_ms > 0 { request.timeout_ms } else { 30_000 });
    let agent = ureq::AgentBuilder::new().redirects(5).timeout(timeout).build();
    let method = if request.method.is_empty() { if request.body.is_some() { "POST" } else { "GET" } } else { request.method.as_str() };
    let mut call = agent.request(method, &request.url);
    for (name, value) in &request.headers {
        call = call.set(name, value);
    }
    if !request.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("user-agent")) {
        call = call.set("User-Agent", "Java-http-client/21");
    }
    let result = match &request.body {
        Some(body) => call.send_bytes(body),
        None => call.call(),
    };
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(problem)) => {
            let class = if problem.kind() == ureq::ErrorKind::Dns { "UnknownHostException" } else { "ConnectException" };
            return Err(exc(class, problem.to_string()));
        }
    };
    let mut headers = Vec::new();
    for name in response.headers_names() {
        for value in response.all(&name) {
            headers.push((name.clone(), value.to_owned()));
        }
    }
    let (status, message) = (i32::from(response.status()), response.status_text().to_owned());
    let mut body = Vec::new();
    response.into_reader().take(64 * 1024 * 1024).read_to_end(&mut body).map_err(|e| exc("IOException", e.to_string()))?;
    Ok(Answer { status, message, headers, body })
}

fn header_of(headers: &[(String, String)], name: &str) -> Option<String> {
    headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}

/// `url.openConnection()`.
pub fn open_connection(url: &str) -> V {
    native(JConnection(std::sync::Mutex::new(Connection { request: Request { url: url.to_owned(), ..Request::default() }, out: None, answer: None })))
}

fn connection_answer(connection: &JConnection) -> Result<Answer, V> {
    let mut state = crate::lock(&connection.0);
    if let Some(answer) = &state.answer {
        return Ok(answer.clone());
    }
    if let Some(out) = state.out.clone() {
        let bytes = crate::call(&out, "toByteArray", &[]).ok().and_then(|b| crate::javaio::bytes_of(&b)).unwrap_or_default();
        state.request.body = Some(bytes);
    }
    let answer = send(&state.request)?;
    state.answer = Some(answer.clone());
    Ok(answer)
}

fn connection_method(connection: &JConnection, this: &V, name: &str, a: &[V]) -> Option<R> {
    let arg = |at: usize| a.get(at).map(V::text).unwrap_or_default();
    let url = || crate::lock(&connection.0).request.url.clone();
    Some(Ok(match name {
        "setRequestMethod" => {
            crate::lock(&connection.0).request.method = arg(0).to_uppercase();
            V::Null
        }
        "getRequestMethod" => {
            let method = crate::lock(&connection.0).request.method.clone();
            crate::s(if method.is_empty() { "GET" } else { &method })
        }
        "setRequestProperty" | "addRequestProperty" => {
            let mut state = crate::lock(&connection.0);
            if name == "setRequestProperty" {
                state.request.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(&arg(0)));
            }
            state.request.headers.push((arg(0), arg(1)));
            V::Null
        }
        "getRequestProperty" => header_of(&crate::lock(&connection.0).request.headers, &arg(0)).map_or(V::Null, |v| crate::s(&v)),
        "setConnectTimeout" | "setReadTimeout" => {
            let ms = a.first().map_or(0, V::long).max(0) as u64;
            let mut state = crate::lock(&connection.0);
            state.request.timeout_ms = state.request.timeout_ms.max(ms);
            V::Null
        }
        "setDoOutput" | "setDoInput" | "setUseCaches" | "setInstanceFollowRedirects" | "setAllowUserInteraction" | "setChunkedStreamingMode"
        | "setFixedLengthStreamingMode" | "setDefaultUseCaches" | "setIfModifiedSince" | "disconnect" => V::Null,
        "connect" => {
            if crate::lock(&connection.0).out.is_none() {
                return Some(connection_answer(connection).map(|_| V::Null));
            }
            V::Null
        }
        "getOutputStream" => {
            let mut state = crate::lock(&connection.0);
            if state.request.method.is_empty() || state.request.method == "GET" {
                state.request.method = "POST".to_owned();
            }
            if state.out.is_none() {
                state.out = Some(crate::new_other("ByteArrayOutputStream", &[]).unwrap_or(V::Null));
            }
            state.out.clone().unwrap_or(V::Null)
        }
        "getResponseCode" => return Some(connection_answer(connection).map(|ans| V::Int(ans.status))),
        "getResponseMessage" => return Some(connection_answer(connection).map(|ans| crate::s(&ans.message))),
        "getInputStream" | "getContent" => {
            return Some(connection_answer(connection).and_then(|ans| {
                if ans.status >= 400 {
                    let class = if ans.status == 404 || ans.status == 410 { "FileNotFoundException" } else { "IOException" };
                    Err(exc(class, format!("Server returned HTTP response code: {} for URL: {}", ans.status, url())))
                } else {
                    Ok(crate::javaio::stream(ans.body))
                }
            }));
        }
        "getErrorStream" => return Some(connection_answer(connection).map(|ans| if ans.status >= 400 { crate::javaio::stream(ans.body) } else { V::Null })),
        "getHeaderField" if matches!(a.first(), Some(V::Str(_))) => {
            return Some(connection_answer(connection).map(|ans| header_of(&ans.headers, &arg(0)).map_or(V::Null, |v| crate::s(&v))));
        }
        "getContentLength" | "getContentLengthLong" => {
            return Some(connection_answer(connection).map(|ans| {
                let n = header_of(&ans.headers, "content-length").and_then(|v| v.parse::<i64>().ok()).unwrap_or(ans.body.len() as i64);
                if name == "getContentLength" { V::Int(n as i32) } else { V::Long(n) }
            }));
        }
        "getContentType" | "getContentEncoding" => {
            let wanted = if name == "getContentType" { "content-type" } else { "content-encoding" };
            return Some(connection_answer(connection).map(|ans| header_of(&ans.headers, wanted).map_or(V::Null, |v| crate::s(&v))));
        }
        "getURL" | "toString" => crate::s(&url()),
        "hashCode" => V::Int(0),
        _ => {
            let _ = this;
            return None;
        }
    }))
}

fn request_of(v: &V) -> Option<Request> {
    v.native::<JHttpPart>().map(|p| crate::lock(&p.0).clone())
}

// ---- com.sun.net.httpserver: a real HTTP/1.1 server, as the JDK's (web panels, OAuth callbacks) ----

use crate::{lock, s};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// `InetSocketAddress`: a host and a port.
pub struct JAddress {
    host: String,
    port: u16,
}

pub struct JHttpServer {
    listener: Mutex<Option<std::net::TcpListener>>,
    contexts: Arc<Mutex<Vec<(String, Arc<Mutex<V>>)>>>,
    stop: Arc<AtomicBool>,
    port: Mutex<u16>,
}

/// `HttpContext`: a path and its handler.
pub struct JContext {
    path: String,
    handler: Arc<Mutex<V>>,
}

/// `Headers`: names compared without case, each with its values.
pub struct JHeaders(Mutex<Vec<(String, String)>>);

struct Exchange {
    method: String,
    uri: String,
    request_headers: V,
    response_headers: V,
    body: Vec<u8>,
    status: Mutex<i32>,
    out: Mutex<Vec<u8>>,
    stream: Mutex<Option<std::net::TcpStream>>,
    remote: (String, u16),
    local_port: u16,
    attributes: Mutex<std::collections::BTreeMap<String, V>>,
}

/// `HttpExchange`, and its response body as an `OutputStream`.
pub struct JExchange(Arc<Exchange>);
pub struct JExchangeBody(Arc<Exchange>);

fn address(host: &str, port: u16) -> V {
    native(JAddress { host: host.to_owned(), port })
}

fn headers(rows: Vec<(String, String)>) -> V {
    native(JHeaders(Mutex::new(rows)))
}

fn reason(code: i32) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "",
    }
}

impl Exchange {
    /// Sends what the handler wrote, once: on `close()`, or when the handler returns.
    fn finish(&self) {
        let Some(mut stream) = lock(&self.stream).take() else { return };
        let status = *lock(&self.status);
        let status = if status <= 0 { 500 } else { status };
        let body = std::mem::take(&mut *lock(&self.out));
        let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
        if let Some(h) = self.response_headers.native::<JHeaders>() {
            for (name, value) in lock(&h.0).iter() {
                if !name.eq_ignore_ascii_case("content-length") && !name.eq_ignore_ascii_case("transfer-encoding") {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
            }
        }
        head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    }
}

/// One connection: its request read, the context's handler run, the response sent.
fn serve(mut stream: std::net::TcpStream, contexts: Arc<Mutex<Vec<(String, Arc<Mutex<V>>)>>>, local_port: u16) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
        if data.len() > 1 << 20 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (method, uri) = (first.next().unwrap_or("GET").to_owned(), first.next().unwrap_or("/").to_owned());
    let rows: Vec<(String, String)> = lines.filter_map(|line| line.split_once(':').map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))).collect();
    let length = rows.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0).min(16 << 20);
    let mut body = data[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    let path = uri.split('?').next().unwrap_or("/").to_owned();
    let handler = lock(&contexts).iter().filter(|(prefix, _)| path.starts_with(prefix.as_str())).max_by_key(|(prefix, _)| prefix.len()).map(|(_, h)| lock(h).clone());
    let remote = stream.peer_addr().map_or((String::from("0.0.0.0"), 0), |a| (a.ip().to_string(), a.port()));
    let exchange = Arc::new(Exchange {
        method,
        uri,
        request_headers: headers(rows),
        response_headers: headers(Vec::new()),
        body,
        status: Mutex::new(0),
        out: Mutex::new(Vec::new()),
        stream: Mutex::new(Some(stream)),
        remote,
        local_port,
        attributes: Mutex::new(std::collections::BTreeMap::new()),
    });
    match handler {
        // No context for the path: the JDK's own 404.
        None | Some(V::Null) => {
            *lock(&exchange.status) = 404;
            lock(&exchange.out).extend_from_slice(b"<h1>404 Not Found</h1>No context found for request");
        }
        Some(handler) => {
            let result = crate::call(&handler, "handle", &[native(JExchange(exchange.clone()))]);
            if let Err(error) = result {
                crate::report("HttpHandler.handle", Err(error));
            }
        }
    }
    exchange.finish();
}

fn server_method(server: &JHttpServer, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match name {
        "bind" => {
            let addr = a.first().and_then(|x| x.native::<JAddress>()).map(|x| (x.host.clone(), x.port));
            match addr {
                Some((host, port)) => match bind(&host, port) {
                    Ok(listener) => {
                        *lock(&server.port) = listener.local_addr().map_or(port, |a| a.port());
                        *lock(&server.listener) = Some(listener);
                        V::Null
                    }
                    Err(error) => return Some(Err(error)),
                },
                None => return Some(Err(exc("NullPointerException", "address"))),
            }
        }
        "createContext" => {
            let path = a.first().map(V::text).unwrap_or_else(|| "/".to_owned());
            let handler = Arc::new(Mutex::new(a.get(1).cloned().unwrap_or(V::Null)));
            lock(&server.contexts).push((path.clone(), handler.clone()));
            native(JContext { path, handler })
        }
        "removeContext" => {
            let path = a.first().map_or_else(String::new, |p| p.native::<JContext>().map_or_else(|| p.text(), |c| c.path.clone()));
            lock(&server.contexts).retain(|(prefix, _)| *prefix != path);
            V::Null
        }
        "setExecutor" => V::Null,
        "getExecutor" => V::Null,
        "getAddress" => address("0.0.0.0", *lock(&server.port)),
        "start" => {
            let Some(listener) = lock(&server.listener).take() else { return Some(Err(exc("IllegalStateException", "server not bound"))) };
            let (contexts, stop, port) = (server.contexts.clone(), server.stop.clone(), *lock(&server.port));
            let _ = listener.set_nonblocking(true);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = stream.set_nonblocking(false);
                            let contexts = contexts.clone();
                            std::thread::spawn(move || serve(stream, contexts, port));
                        }
                        Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
                    }
                }
            });
            V::Null
        }
        "stop" => {
            server.stop.store(true, Ordering::Relaxed);
            lock(&server.listener).take();
            V::Null
        }
        "hashCode" => V::Int(*lock(&server.port) as i32),
        "toString" => s("HttpServer"),
        _ => {
            let _ = this;
            return None;
        }
    }))
}

fn bind(host: &str, port: u16) -> Result<std::net::TcpListener, V> {
    let host = if host.is_empty() || host == "*" { "0.0.0.0" } else { host };
    std::net::TcpListener::bind((host, port)).map_err(|e| exc("BindException", format!("{e}")))
}

fn header_method(h: &JHeaders, name: &str, a: &[V]) -> Option<R> {
    let key = || a.first().map(V::text).unwrap_or_default();
    Some(Ok(match (name, a.len()) {
        ("getFirst", 1) => lock(&h.0).iter().find(|(k, _)| k.eq_ignore_ascii_case(&key())).map_or(V::Null, |(_, v)| s(v)),
        ("get", 1) => {
            let found: Vec<V> = lock(&h.0).iter().filter(|(k, _)| k.eq_ignore_ascii_case(&key())).map(|(_, v)| s(v)).collect();
            if found.is_empty() { V::Null } else { crate::list(found) }
        }
        ("containsKey", 1) => V::Bool(lock(&h.0).iter().any(|(k, _)| k.eq_ignore_ascii_case(&key()))),
        ("add", 2) => {
            lock(&h.0).push((key(), a[1].text()));
            V::Null
        }
        ("set", 2) => {
            let mut rows = lock(&h.0);
            rows.retain(|(k, _)| !k.eq_ignore_ascii_case(&key()));
            rows.push((key(), a[1].text()));
            V::Null
        }
        ("put", 2) => {
            let mut rows = lock(&h.0);
            rows.retain(|(k, _)| !k.eq_ignore_ascii_case(&key()));
            for value in a[1].items().unwrap_or_default() {
                rows.push((key(), value.text()));
            }
            V::Null
        }
        ("remove", 1) => {
            lock(&h.0).retain(|(k, _)| !k.eq_ignore_ascii_case(&key()));
            V::Null
        }
        ("isEmpty", 0) => V::Bool(lock(&h.0).is_empty()),
        ("size", 0) => V::Int(lock(&h.0).len() as i32),
        ("keySet", 0) => crate::call_static("Set", "copyOf", &[crate::list(lock(&h.0).iter().map(|(k, _)| s(k)).collect())]).unwrap_or(V::Null),
        _ => return None,
    }))
}

fn exchange_method(x: &Arc<Exchange>, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("getRequestMethod", 0) => s(&x.method),
        ("getRequestURI", 0) => s(&x.uri),
        ("getRequestHeaders", 0) => x.request_headers.clone(),
        ("getResponseHeaders", 0) => x.response_headers.clone(),
        ("getRequestBody", 0) => crate::javaio::stream(x.body.clone()),
        ("getResponseBody", 0) => native(JExchangeBody(x.clone())),
        ("sendResponseHeaders", 2) => {
            *lock(&x.status) = a[0].int();
            V::Null
        }
        ("getResponseCode", 0) => V::Int(*lock(&x.status)),
        ("getRemoteAddress", 0) => address(&x.remote.0, x.remote.1),
        ("getLocalAddress", 0) => address("127.0.0.1", x.local_port),
        ("getProtocol", 0) => s("HTTP/1.1"),
        ("getAttribute", 1) => lock(&x.attributes).get(&a[0].text()).cloned().unwrap_or(V::Null),
        ("setAttribute", 2) => {
            lock(&x.attributes).insert(a[0].text(), a[1].clone());
            V::Null
        }
        ("close", 0) => {
            x.finish();
            V::Null
        }
        _ => return None,
    }))
}

fn body_method(x: &Arc<Exchange>, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match name {
        "write" => {
            let bytes = match a {
                [V::Int(b)] => vec![*b as u8],
                [data] => crate::javaio::bytes_of(data).unwrap_or_else(|| data.text().into_bytes()),
                [data, from, count] => {
                    let all = crate::javaio::bytes_of(data).unwrap_or_default();
                    let from = (from.int().max(0) as usize).min(all.len());
                    all[from..(from + count.int().max(0) as usize).min(all.len())].to_vec()
                }
                _ => Vec::new(),
            };
            lock(&x.out).extend_from_slice(&bytes);
            V::Null
        }
        "flush" => V::Null,
        "close" => {
            x.finish();
            V::Null
        }
        _ => return None,
    }))
}

/// `URI` methods on a URI's text: what a handler reads of `getRequestURI()`.
fn uri_part(text: &str, name: &str) -> Option<V> {
    let (before_fragment, fragment) = text.split_once('#').map_or((text, None), |(a, b)| (a, Some(b)));
    let (before_query, query) = before_fragment.split_once('?').map_or((before_fragment, None), |(a, b)| (a, Some(b)));
    let (scheme, rest) = before_query.split_once("://").map_or((None, before_query), |(a, b)| (Some(a), b));
    let (authority, path) = if scheme.is_some() { rest.split_once('/').map_or((Some(rest), String::new()), |(a, b)| (Some(a), format!("/{b}"))) } else { (None, rest.to_owned()) };
    let decode = |t: &str| crate::call_static("URLDecoder", "decode", &[s(t), s("UTF-8")]).map_or_else(|_| t.to_owned(), |v| v.text());
    let opt = |v: Option<&str>| v.map_or(V::Null, s);
    Some(match name {
        "getPath" => s(&decode(&path)),
        "getRawPath" => s(&path),
        "getQuery" => query.map_or(V::Null, |q| s(&decode(q))),
        "getRawQuery" => opt(query),
        "getFragment" | "getRawFragment" => opt(fragment),
        "getScheme" => opt(scheme),
        "getHost" => authority.map_or(V::Null, |a| s(a.rsplit_once(':').map_or(a, |(h, _)| h))),
        "getPort" => V::Int(authority.and_then(|a| a.rsplit_once(':')).and_then(|(_, p)| p.parse().ok()).unwrap_or(-1)),
        "getAuthority" | "getRawAuthority" => opt(authority),
        _ => return None,
    })
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(Ok(match (simple, a.len()) {
        ("InetSocketAddress", 1) => address("0.0.0.0", a[0].int() as u16),
        ("InetSocketAddress", 2) => address(&a[0].text(), a[1].int() as u16),
        _ => return None,
    }))
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    if simple == "HttpServer" && name == "create" {
        let server = JHttpServer { listener: Mutex::new(None), contexts: Arc::new(Mutex::new(Vec::new())), stop: Arc::new(AtomicBool::new(false)), port: Mutex::new(0) };
        let made = native(server);
        if let Some(addr) = a.first().filter(|x| !x.is_null()) {
            if let Err(error) = crate::call(&made, "bind", &[addr.clone(), V::Int(0)]) {
                return Some(Err(error));
            }
        }
        return Some(Ok(made));
    }
    if simple == "InetSocketAddress" && name == "createUnresolved" && a.len() == 2 {
        return Some(Ok(address(&a[0].text(), a[1].int() as u16)));
    }
    Some(Ok(match (simple, name) {
        ("HttpClient", "newHttpClient" | "newBuilder") => native(JHttpClient),
        ("HttpRequest", "newBuilder") => native(JHttpPart(Mutex::new(Request { url: a.first().map(V::text).unwrap_or_default(), ..Request::default() }))),
        ("BodyPublishers", "ofString" | "ofByteArray") => native(JBodyPart(a.first().map(|x| crate::javaio::bytes_of(x).unwrap_or_else(|| x.text().into_bytes())), "body")),
        ("BodyPublishers", "noBody") => native(JBodyPart(None, "body")),
        ("BodyHandlers", "ofString" | "ofLines") => native(JBodyPart(None, "string")),
        ("BodyHandlers", "ofByteArray") => native(JBodyPart(None, "bytes")),
        ("BodyHandlers", "ofInputStream") => native(JBodyPart(None, "stream")),
        ("BodyHandlers", "discarding" | "replacing") => native(JBodyPart(None, "none")),
        ("BodyPublishers" | "BodyHandlers" | "BodySubscribers", _) => native(JBodyPart(None, "string")),
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    // `URI.create(text)` is its text here: `uri.toURL().openConnection()` meets the missing network.
    if let V::Str(text) = this {
        return match name {
            "toURL" => Some(Ok(this.clone())),
            "openConnection" if text.starts_with("jar:") => Some(Ok(crate::javacore::jar_connection(text))),
            "openConnection" => Some(Ok(open_connection(text))),
            "openStream" => Some(send(&Request { url: text.to_string(), ..Request::default() }).and_then(|a| {
                if a.status >= 400 { Err(exc("IOException", format!("Server returned HTTP response code: {} for URL: {text}", a.status))) } else { Ok(crate::javaio::stream(a.body)) }
            })),
            _ if a.is_empty() => uri_part(text, name).map(Ok),
            _ => None,
        };
    }
    if let Some(connection) = this.native::<JConnection>() {
        return connection_method(connection, this, name, a);
    }
    if let Some(server) = this.native::<JHttpServer>() {
        return server_method(server, this, name, a);
    }
    if let Some(x) = this.native::<JExchange>() {
        return exchange_method(&x.0, name, a);
    }
    if let Some(x) = this.native::<JExchangeBody>() {
        return body_method(&x.0, name, a);
    }
    if let Some(h) = this.native::<JHeaders>() {
        return header_method(h, name, a);
    }
    if let Some(c) = this.native::<JContext>() {
        return Some(Ok(match (name, a.len()) {
            ("setHandler", 1) => {
                *lock(&c.handler) = a[0].clone();
                V::Null
            }
            ("getHandler", 0) => lock(&c.handler).clone(),
            ("getPath", 0) => s(&c.path),
            ("getFilters", 0) => crate::list(Vec::new()),
            ("getAttributes", 0) => V::Map(Arc::new(Mutex::new(crate::JMap::new()))),
            _ => return None,
        }));
    }
    if let Some(addr) = this.native::<JAddress>() {
        return Some(Ok(match name {
            "getPort" => V::Int(i32::from(addr.port)),
            "getHostString" | "getHostName" => s(&addr.host),
            "getAddress" => this.clone(),
            "getHostAddress" => s(&addr.host),
            "toString" => s(&format!("/{}:{}", addr.host, addr.port)),
            "isUnresolved" => V::Bool(false),
            "hashCode" => V::Int(i32::from(addr.port)),
            _ => return None,
        }));
    }
    if this.native::<JHttpClient>().is_some() {
        let respond = || -> R {
            let request = a.first().and_then(request_of).ok_or_else(|| exc("NullPointerException", "request"))?;
            let kind = a.get(1).and_then(|h| h.native::<JBodyPart>().map(|b| b.1)).unwrap_or("string");
            let answer = send(&request)?;
            Ok(native(JHttpResponse(answer, request.url.clone(), kind)))
        };
        return Some(match name {
            "send" => respond(),
            "sendAsync" => match respond() {
                Ok(response) => crate::call_static("CompletableFuture", "completedFuture", &[response]),
                Err(thrown) => crate::call_static("CompletableFuture", "failedFuture", &[thrown]),
            },
            "close" | "shutdown" | "shutdownNow" => Ok(V::Null),
            "awaitTermination" | "isTerminated" => Ok(V::Bool(true)),
            "hashCode" => Ok(V::Int(0)),
            "equals" => Ok(V::Bool(a.first().is_some_and(|o| o.native::<JHttpClient>().is_some()))),
            "toString" => Ok(crate::s("HttpClient")),
            // The builder's settings, and what the client is asked about itself.
            _ => Ok(this.clone()),
        });
    }
    if let Some(part) = this.native::<JHttpPart>() {
        let arg = |at: usize| a.get(at).map(V::text).unwrap_or_default();
        let mut request = lock(&part.0);
        match (name, a.len()) {
            ("uri", 1) => request.url = arg(0),
            ("uri", 0) => return Some(Ok(s(&request.url))),
            ("header" | "setHeader", 2) => {
                if name == "setHeader" {
                    request.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(&arg(0)));
                }
                request.headers.push((arg(0), arg(1)));
            }
            ("headers", n) if n >= 2 => {
                for pair in a.chunks(2) {
                    request.headers.push((pair[0].text(), pair.get(1).map(V::text).unwrap_or_default()));
                }
            }
            ("GET" | "DELETE", 0) => {
                request.method = name.to_owned();
                request.body = None;
            }
            ("POST" | "PUT", 1) => {
                request.method = name.to_owned();
                request.body = a[0].native::<JBodyPart>().and_then(|b| b.0.clone());
            }
            ("method", 2) => {
                request.method = arg(0).to_uppercase();
                request.body = a[1].native::<JBodyPart>().and_then(|b| b.0.clone());
            }
            ("timeout", 1) => request.timeout_ms = a[0].native::<crate::javaio::JDuration>().map_or(30_000, |d| d.0.max(0) as u64),
            ("method", 0) => return Some(Ok(s(if request.method.is_empty() { "GET" } else { &request.method }))),
            ("build", 0) => return Some(Ok(native(JHttpPart(Mutex::new(request.clone()))))),
            ("hashCode", 0) => return Some(Ok(V::Int(0))),
            ("equals", 1) => return Some(Ok(V::Bool(false))),
            ("toString", 0) => return Some(Ok(s(&format!("{} {}", if request.method.is_empty() { "GET" } else { &request.method }, request.url)))),
            _ => {}
        }
        return Some(Ok(this.clone()));
    }
    if let Some(response) = this.native::<JHttpResponse>() {
        let JHttpResponse(answer, url, kind) = response;
        return Some(Ok(match name {
            "statusCode" => V::Int(answer.status),
            "body" => match *kind {
                "bytes" => crate::javaio::to_byte_array(&answer.body),
                "stream" => crate::javaio::stream(answer.body.clone()),
                "none" => V::Null,
                _ => s(&String::from_utf8_lossy(&answer.body)),
            },
            "headers" => native(JResponseHeaders(answer.headers.clone())),
            "uri" => s(url),
            "version" => s("HTTP_1_1"),
            _ => return None,
        }));
    }
    if let Some(headers) = this.native::<JResponseHeaders>() {
        let arg = a.first().map(V::text).unwrap_or_default();
        return Some(match name {
            "firstValue" => crate::call_static("Optional", "ofNullable", &[header_of(&headers.0, &arg).map_or(V::Null, |v| s(&v))]),
            "allValues" => Ok(crate::list(headers.0.iter().filter(|(n, _)| n.eq_ignore_ascii_case(&arg)).map(|(_, v)| s(v)).collect())),
            "firstValueAsLong" => Ok(V::Long(header_of(&headers.0, &arg).and_then(|v| v.parse().ok()).unwrap_or(0))),
            _ => return None,
        });
    }
    if this.native::<JBodyPart>().is_some() {
        return Some(Ok(this.clone()));
    }

    None
}
