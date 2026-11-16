//! OkHttp (and the bit of okio it hands plugins), native: a plugin's bundled OkHttp is dropped by jport (its copy opens
//! raw sockets and does TLS itself, which a converted plugin can't), and its calls land here, sent by jrt's own HTTP
//! client (`net::send`: ureq, real TLS, redirects). The API plugins use: `OkHttpClient` (+ `newBuilder`, interceptors
//! and network interceptors, timeouts), `Request.Builder`, `RequestBody`/`ResponseBody.create`, `MediaType.get`,
//! `Call.execute` / `enqueue(Callback)` (a thread of its own, as OkHttp's dispatcher), `Response`, `Interceptor.Chain`,
//! and okio's `Buffer` as `RequestBody.writeTo` fills it. Tebex (Buycraft) talks to its API through all of these.
use crate::{R, V, arr, call, exc, list, lock, native, s};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct Client {
    interceptors: Vec<V>,
    network: Vec<V>,
    timeout_ms: u64,
}

pub struct JClient(Client);
pub struct JClientBuilder(Mutex<Client>);

#[derive(Clone, Default)]
pub struct Request {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<V>,
}

pub struct JRequest(Request);
pub struct JRequestBuilder(Mutex<Request>);

/// `RequestBody` / `ResponseBody`: the bytes and their media type.
pub struct JBody {
    bytes: Vec<u8>,
    media: Option<String>,
}

pub struct JMediaType(String);
pub struct JUrl(String);

pub struct JResponse {
    code: i32,
    message: String,
    headers: Vec<(String, String)>,
    body: V,
    request: V,
}

pub struct JResponseBuilder(Mutex<(i32, String, Vec<(String, String)>, V, V)>);

pub struct JCall {
    client: Client,
    request: V,
    executed: Mutex<bool>,
    cancelled: Mutex<bool>,
}

pub struct JChain {
    client: Client,
    request: V,
    at: usize,
}

pub struct JHeaders(Vec<(String, String)>);
/// okio's `Buffer`, as far as `RequestBody.writeTo(buffer)` and reading it back go.
pub struct JBuffer(Mutex<Vec<u8>>);

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}

fn millis(amount: &V, unit: Option<&V>) -> u64 {
    // `connectTimeout(10, TimeUnit.SECONDS)` or `connectTimeout(Duration.ofSeconds(10))`.
    if unit.is_none() && matches!(amount, V::Native(_)) {
        return call(amount, "toMillis", &[]).map_or(0, |v| v.long().max(0) as u64);
    }
    let n = amount.long().max(0) as u64;
    match unit.map(V::text).unwrap_or_default().as_str() {
        "NANOSECONDS" => n / 1_000_000,
        "MICROSECONDS" => n / 1000,
        "SECONDS" => n * 1000,
        "MINUTES" => n * 60_000,
        "HOURS" => n * 3_600_000,
        _ => n,
    }
}

fn body_of(content: &V, media: Option<String>) -> V {
    let bytes = match content {
        V::Arr(items) => lock(items).iter().map(|b| b.int() as u8).collect(),
        other => other.text().into_bytes(),
    };
    native(JBody { bytes, media })
}

fn media_of(value: Option<&V>) -> Option<String> {
    value.filter(|v| !v.is_null()).map(|v| v.native::<JMediaType>().map_or_else(|| v.text(), |m| m.0.clone()))
}

/// `RequestBody.create(...)` / `ResponseBody.create(...)`: (content, mediaType) or the older (mediaType, content).
fn create_body(a: &[V]) -> V {
    match a {
        [first, second, ..] if first.native::<JMediaType>().is_some() || (first.is_null() && !second.is_null()) => body_of(second, media_of(Some(first))),
        [content, media, ..] => body_of(content, media_of(Some(media))),
        [content] => body_of(content, None),
        [] => body_of(&s(""), None),
    }
}

/// The request through the interceptors (the client's, then the network ones) and over the wire.
fn proceed(client: &Client, request: &V, at: usize) -> R {
    let chain_count = client.interceptors.len() + client.network.len();
    if at < chain_count {
        let interceptor = if at < client.interceptors.len() { &client.interceptors[at] } else { &client.network[at - client.interceptors.len()] };
        let chain = native(JChain { client: client.clone(), request: request.clone(), at: at + 1 });
        return call(interceptor, "intercept", &[chain]);
    }
    let Some(r) = request.native::<JRequest>() else { return Err(exc("IllegalStateException", "not a request")) };
    let req = &r.0;
    let (bytes, media) = match req.body.as_ref().and_then(|b| b.native::<JBody>().map(|b| (b.bytes.clone(), b.media.clone()))) {
        Some((bytes, media)) => (Some(bytes), media),
        None => (None, None),
    };
    let mut headers = req.headers.clone();
    if let Some(media) = media.filter(|_| header(&headers, "Content-Type").is_none()) {
        headers.push(("Content-Type".to_owned(), media));
    }
    if header(&headers, "User-Agent").is_none() {
        headers.push(("User-Agent".to_owned(), "okhttp/4.12.0".to_owned()));
    }
    let method = if req.method.is_empty() { "GET".to_owned() } else { req.method.clone() };
    let sent = crate::net::Request { url: req.url.clone(), method, headers, body: bytes, timeout_ms: client.timeout_ms };
    let answer = crate::net::send(&sent).map_err(|e| exc("IOException", e.text()))?;
    let media = header(&answer.headers, "Content-Type");
    Ok(native(JResponse {
        code: answer.status,
        message: answer.message.clone(),
        headers: answer.headers.clone(),
        body: native(JBody { bytes: answer.body.clone(), media }),
        request: request.clone(),
    }))
}

fn request_builder_method(b: &JRequestBuilder, this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut req = lock(&b.0);
    let text = |i: usize| a.get(i).map(V::text).unwrap_or_default();
    Some(Ok(match (name, a.len()) {
        ("url", 1) => {
            req.url = a[0].native::<JUrl>().map_or_else(|| a[0].text(), |u| u.0.clone());
            this.clone()
        }
        ("header", 2) => {
            req.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(&text(0)));
            req.headers.push((text(0), text(1)));
            this.clone()
        }
        ("addHeader", 2) => {
            req.headers.push((text(0), text(1)));
            this.clone()
        }
        ("removeHeader", 1) => {
            req.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(&text(0)));
            this.clone()
        }
        ("headers", 1) => {
            req.headers = a[0].native::<JHeaders>().map(|h| h.0.clone()).unwrap_or_default();
            this.clone()
        }
        ("method", 2) => {
            req.method = text(0).to_uppercase();
            req.body = (!a[1].is_null()).then(|| a[1].clone());
            this.clone()
        }
        ("get", 0) | ("head", 0) => {
            req.method = if name == "get" { "GET".to_owned() } else { "HEAD".to_owned() };
            req.body = None;
            this.clone()
        }
        ("post" | "put" | "patch", 1) => {
            req.method = name.to_uppercase();
            req.body = Some(a[0].clone());
            this.clone()
        }
        ("delete", 0 | 1) => {
            req.method = "DELETE".to_owned();
            req.body = a.first().filter(|b| !b.is_null()).cloned();
            this.clone()
        }
        ("tag" | "cacheControl", _) => this.clone(),
        ("build", 0) => {
            if req.url.is_empty() {
                return Some(Err(exc("IllegalStateException", "url == null")));
            }
            let mut built = req.clone();
            if built.method.is_empty() {
                built.method = "GET".to_owned();
            }
            native(JRequest(built))
        }
        _ => return None,
    }))
}

fn request_method(r: &JRequest, name: &str, a: &[V]) -> Option<R> {
    let req = &r.0;
    Some(Ok(match (name, a.len()) {
        ("method", 0) => s(&req.method),
        ("url", 0) => native(JUrl(req.url.clone())),
        ("header", 1) => header(&req.headers, &a[0].text()).map_or(V::Null, |v| s(&v)),
        ("headers", 0) => native(JHeaders(req.headers.clone())),
        ("headers", 1) => list(req.headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case(&a[0].text())).map(|(_, v)| s(v)).collect()),
        ("body", 0) => req.body.clone().unwrap_or(V::Null),
        ("newBuilder", 0) => native(JRequestBuilder(Mutex::new(req.clone()))),
        ("isHttps", 0) => V::Bool(req.url.starts_with("https://")),
        ("toString", 0) => s(&format!("Request{{method={}, url={}}}", req.method, req.url)),
        _ => return None,
    }))
}

fn body_method(b: &JBody, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("string", 0) => s(&String::from_utf8_lossy(&b.bytes)),
        ("bytes", 0) => arr(b.bytes.iter().map(|&x| V::Int(x as i8 as i32)).collect()),
        ("contentType", 0) => b.media.as_ref().map_or(V::Null, |m| native(JMediaType(m.clone()))),
        ("contentLength", 0) => V::Long(b.bytes.len() as i64),
        ("byteStream", 0) | ("charStream", 0) => {
            let bytes = arr(b.bytes.iter().map(|&x| V::Int(x as i8 as i32)).collect());
            return Some(crate::new_other("ByteArrayInputStream", &[bytes]));
        }
        // `requestBody.writeTo(buffer)`: its bytes into the okio Buffer.
        ("writeTo", 1) => {
            if let Some(buffer) = a[0].native::<JBuffer>() {
                lock(&buffer.0).extend_from_slice(&b.bytes);
            }
            V::Null
        }
        ("close", 0) => V::Null,
        _ => return None,
    }))
}

fn response_method(r: &JResponse, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("code", 0) => V::Int(r.code),
        ("message", 0) => s(&r.message),
        ("isSuccessful", 0) => V::Bool((200..300).contains(&r.code)),
        ("isRedirect", 0) => V::Bool(matches!(r.code, 300 | 301 | 302 | 303 | 307 | 308)),
        ("body", 0) => r.body.clone(),
        ("header", 1 | 2) => header(&r.headers, &a[0].text()).map_or_else(|| a.get(1).cloned().unwrap_or(V::Null), |v| s(&v)),
        ("headers", 0) => native(JHeaders(r.headers.clone())),
        ("headers", 1) => list(r.headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case(&a[0].text())).map(|(_, v)| s(v)).collect()),
        ("request", 0) => r.request.clone(),
        ("protocol", 0) => s("HTTP_1_1"),
        ("close", 0) => V::Null,
        ("newBuilder", 0) => native(JResponseBuilder(Mutex::new((r.code, r.message.clone(), r.headers.clone(), r.body.clone(), r.request.clone())))),
        ("toString", 0) => s(&format!("Response{{protocol=http/1.1, code={}, message={}, url={}}}", r.code, r.message, r.request.native::<JRequest>().map_or_else(String::new, |q| q.0.url.clone()))),
        _ => {
            let _ = this;
            return None;
        }
    }))
}

fn response_builder_method(b: &JResponseBuilder, this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut parts = lock(&b.0);
    Some(Ok(match (name, a.len()) {
        ("body", 1) => {
            parts.3 = a[0].clone();
            this.clone()
        }
        ("code", 1) => {
            parts.0 = a[0].int();
            this.clone()
        }
        ("message", 1) => {
            parts.1 = a[0].text();
            this.clone()
        }
        ("header" | "addHeader", 2) => {
            if name == "header" {
                parts.2.retain(|(n, _)| !n.eq_ignore_ascii_case(&a[0].text()));
            }
            parts.2.push((a[0].text(), a[1].text()));
            this.clone()
        }
        ("request", 1) => {
            parts.4 = a[0].clone();
            this.clone()
        }
        ("build", 0) => native(JResponse { code: parts.0, message: parts.1.clone(), headers: parts.2.clone(), body: parts.3.clone(), request: parts.4.clone() }),
        _ => return None,
    }))
}

fn call_method(c: &Arc<JCall>, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(match (name, a.len()) {
        ("request", 0) => Ok(c.request.clone()),
        ("execute", 0) => {
            *lock(&c.executed) = true;
            if *lock(&c.cancelled) {
                return Some(Err(exc("IOException", "Canceled")));
            }
            proceed(&c.client, &c.request, 0)
        }
        // `call.enqueue(callback)`: on a thread of its own; `onResponse(call, response)` or `onFailure(call, e)` there,
        // as OkHttp's dispatcher calls them.
        ("enqueue", 1) => {
            *lock(&c.executed) = true;
            let (client, request, callback, me) = (c.client.clone(), c.request.clone(), a[0].clone(), this.clone());
            let cancelled = *lock(&c.cancelled);
            std::thread::Builder::new()
                .name("OkHttp Dispatcher".to_owned())
                .stack_size(64 << 20)
                .spawn(move || {
                    let outcome = if cancelled { Err(exc("IOException", "Canceled")) } else { proceed(&client, &request, 0) };
                    let answered = match outcome {
                        Ok(response) => call(&callback, "onResponse", &[me.clone(), response]),
                        Err(problem) => {
                            let problem = if crate::instance_of(&problem, "IOException") { problem } else { exc("IOException", problem.text()) };
                            call(&callback, "onFailure", &[me.clone(), problem])
                        }
                    };
                    let _ = crate::report("OkHttp callback", answered);
                })
                .map(|_| V::Null)
                .map_err(|e| exc("IOException", e.to_string()))
        }
        ("cancel", 0) => {
            *lock(&c.cancelled) = true;
            Ok(V::Null)
        }
        ("isCanceled", 0) => Ok(V::Bool(*lock(&c.cancelled))),
        ("isExecuted", 0) => Ok(V::Bool(*lock(&c.executed))),
        ("clone", 0) => Ok(native(Arc::new(JCall { client: c.client.clone(), request: c.request.clone(), executed: Mutex::new(false), cancelled: Mutex::new(false) }))),
        _ => return None,
    })
}

// ---- The hooks jrt calls ----

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match class {
        "OkHttpClient" => native(JClient(Client { timeout_ms: 10_000, ..Client::default() })),
        "OkHttpClient.Builder" => native(JClientBuilder(Mutex::new(Client { timeout_ms: 10_000, ..Client::default() }))),
        "Request.Builder" => native(JRequestBuilder(Mutex::new(Request { method: "GET".to_owned(), ..Request::default() }))),
        "Response.Builder" => native(JResponseBuilder(Mutex::new((200, "OK".to_owned(), Vec::new(), V::Null, V::Null)))),
        // okio's Buffer (`new Buffer()` for `requestBody.writeTo(buffer)`).
        "Buffer" if a.is_empty() => native(JBuffer(Mutex::new(Vec::new()))),
        _ => return None,
    }))
}

pub fn field(class: &str, name: &str) -> Option<V> {
    // Kotlin's companions, as Java code reaches them: `RequestBody.Companion.create(...)`.
    (name == "Companion" && matches!(class, "RequestBody" | "ResponseBody" | "MediaType" | "HttpUrl")).then(|| V::Class(match class {
        "RequestBody" => "RequestBody",
        "ResponseBody" => "ResponseBody",
        "MediaType" => "MediaType",
        _ => "HttpUrl",
    }))
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, name) {
        ("RequestBody" | "ResponseBody", "create") => create_body(a),
        ("MediaType", "get" | "parse") => native(JMediaType(a.first().map(V::text).unwrap_or_default())),
        ("HttpUrl", "get" | "parse") => native(JUrl(a.first().map(V::text).unwrap_or_default())),
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let V::Native(_) = this else {
        // A Kotlin companion reached as a class: `RequestBody.Companion.create(...)`.
        if let V::Class(class @ ("RequestBody" | "ResponseBody" | "MediaType" | "HttpUrl")) = this {
            return static_call(class, name, a);
        }
        return None;
    };
    if let Some(client) = this.native::<JClient>() {
        return Some(Ok(match (name, a.len()) {
            ("newBuilder", 0) => native(JClientBuilder(Mutex::new(client.0.clone()))),
            ("newCall", 1) => native(Arc::new(JCall { client: client.0.clone(), request: a[0].clone(), executed: Mutex::new(false), cancelled: Mutex::new(false) })),
            ("interceptors", 0) => list(client.0.interceptors.clone()),
            ("networkInterceptors", 0) => list(client.0.network.clone()),
            ("connectTimeoutMillis" | "readTimeoutMillis" | "writeTimeoutMillis" | "callTimeoutMillis", 0) => V::Int(client.0.timeout_ms as i32),
            ("dispatcher" | "connectionPool" | "cache", 0) => native(JClient(client.0.clone())),
            // `client.dispatcher().executorService().shutdown()`, `connectionPool().evictAll()`: nothing kept to stop.
            ("executorService", 0) => this.clone(),
            ("evictAll" | "cancelAll" | "shutdown" | "close", 0) => V::Null,
            _ => return None,
        }));
    }
    if let Some(builder) = this.native::<JClientBuilder>() {
        let mut client = lock(&builder.0);
        return Some(Ok(match (name, a.len()) {
            ("addInterceptor", 1) => {
                client.interceptors.push(a[0].clone());
                this.clone()
            }
            ("addNetworkInterceptor", 1) => {
                client.network.push(a[0].clone());
                this.clone()
            }
            ("connectTimeout" | "readTimeout" | "writeTimeout" | "callTimeout", 1 | 2) => {
                let ms = millis(&a[0], a.get(1));
                if ms > 0 {
                    client.timeout_ms = client.timeout_ms.max(ms);
                }
                this.clone()
            }
            ("retryOnConnectionFailure" | "followRedirects" | "followSslRedirects" | "dispatcher" | "connectionPool" | "cache" | "proxy" | "dns"
            | "protocols" | "connectionSpecs" | "eventListener" | "cookieJar" | "pingInterval" | "hostnameVerifier" | "sslSocketFactory", _) => this.clone(),
            ("build", 0) => native(JClient(client.clone())),
            _ => return None,
        }));
    }
    if let Some(b) = this.native::<JRequestBuilder>() {
        return request_builder_method(b, this, name, a);
    }
    if let Some(r) = this.native::<JRequest>() {
        return request_method(r, name, a);
    }
    if let Some(c) = this.native::<Arc<JCall>>() {
        return call_method(c, this, name, a);
    }
    if let Some(chain) = this.native::<JChain>() {
        return Some(match (name, a.len()) {
            ("request", 0) => Ok(chain.request.clone()),
            ("proceed", 1) => proceed(&chain.client, &a[0], chain.at),
            ("call", 0) => Ok(native(Arc::new(JCall { client: chain.client.clone(), request: chain.request.clone(), executed: Mutex::new(true), cancelled: Mutex::new(false) }))),
            ("connectTimeoutMillis" | "readTimeoutMillis" | "writeTimeoutMillis", 0) => Ok(V::Int(chain.client.timeout_ms as i32)),
            _ => return None,
        });
    }
    if let Some(r) = this.native::<JResponse>() {
        return response_method(r, this, name, a);
    }
    if let Some(b) = this.native::<JResponseBuilder>() {
        return response_builder_method(b, this, name, a);
    }
    if let Some(b) = this.native::<JBody>() {
        return body_method(b, name, a);
    }
    if let Some(m) = this.native::<JMediaType>() {
        let (kind, rest) = m.0.split_once('/').unwrap_or((m.0.as_str(), ""));
        return Some(Ok(match name {
            "toString" => s(&m.0),
            "type" => s(kind.trim()),
            "subtype" => s(rest.split(';').next().unwrap_or("").trim()),
            "charset" => {
                let named = m.0.split(';').find_map(|p| p.trim().strip_prefix("charset=").map(|c| c.trim_matches('"').to_owned()));
                match named.or_else(|| a.first().filter(|d| !d.is_null()).map(V::text)) {
                    Some(c) => s(&c),
                    None => V::Null,
                }
            }
            _ => return None,
        }));
    }
    if let Some(u) = this.native::<JUrl>() {
        let after = u.0.split_once("://").map_or(u.0.as_str(), |(_, r)| r);
        return Some(Ok(match name {
            "toString" | "url" | "toUri" => s(&u.0),
            "scheme" => s(u.0.split_once("://").map_or("", |(sc, _)| sc)),
            "host" => s(after.split(['/', ':', '?']).next().unwrap_or("")),
            "encodedPath" => s(&format!("/{}", after.split_once('/').map_or("", |(_, p)| p).split('?').next().unwrap_or(""))),
            "isHttps" => V::Bool(u.0.starts_with("https://")),
            _ => return None,
        }));
    }
    if let Some(h) = this.native::<JHeaders>() {
        return Some(Ok(match (name, a.len()) {
            ("get", 1) => header(&h.0, &a[0].text()).map_or(V::Null, |v| s(&v)),
            ("size", 0) => V::Int(h.0.len() as i32),
            ("name", 1) => h.0.get(a[0].int().max(0) as usize).map_or(V::Null, |(n, _)| s(n)),
            ("value", 1) => h.0.get(a[0].int().max(0) as usize).map_or(V::Null, |(_, v)| s(v)),
            ("names", 0) => list(h.0.iter().map(|(n, _)| s(n)).collect()),
            ("values", 1) => list(h.0.iter().filter(|(n, _)| n.eq_ignore_ascii_case(&a[0].text())).map(|(_, v)| s(v)).collect()),
            ("toString", 0) => s(&h.0.iter().map(|(n, v)| format!("{n}: {v}\n")).collect::<String>()),
            _ => return None,
        }));
    }
    if let Some(buffer) = this.native::<JBuffer>() {
        let mut bytes = lock(&buffer.0);
        return Some(Ok(match (name, a.len()) {
            ("readString" | "readUtf8", _) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                bytes.clear();
                s(&text)
            }
            ("writeUtf8", 1) | ("writeString", _) => {
                bytes.extend_from_slice(a[0].text().as_bytes());
                this.clone()
            }
            ("size", 0) => V::Long(bytes.len() as i64),
            ("clear", 0) => {
                bytes.clear();
                V::Null
            }
            ("readByteArray", 0) => {
                let out = arr(bytes.iter().map(|&x| V::Int(x as i8 as i32)).collect());
                bytes.clear();
                out
            }
            ("close" | "flush", 0) => V::Null,
            _ => return None,
        }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(r: Option<R>) -> V {
        match r {
            Some(Ok(v)) => v,
            Some(Err(e)) => panic!("threw {}", e.text()),
            None => panic!("no value"),
        }
    }

    #[test]
    fn a_request_is_built_and_rebuilt_as_okhttp_does() {
        let b = ok(construct("Request.Builder", &[]));
        ok(method(&b, "url", &[s("https://plugin.tebex.io/information")]));
        ok(method(&b, "addHeader", &[s("X-Tebex-Secret"), s("abc")]));
        let media = ok(static_call("MediaType", "get", &[s("application/json; charset=utf-8")]));
        let body = ok(static_call("RequestBody", "create", &[s("{\"a\":1}"), media]));
        ok(method(&b, "method", &[s("post"), body]));
        let req = ok(method(&b, "build", &[]));
        assert_eq!(ok(method(&req, "method", &[])).text(), "POST");
        assert_eq!(ok(method(&req, "header", &[s("x-tebex-secret")])).text(), "abc");
        let again = ok(method(&ok(method(&req, "newBuilder", &[])), "header", &[s("User-Agent"), s("Minecraft-PAPER")]));
        let rebuilt = ok(method(&again, "build", &[]));
        assert_eq!(ok(method(&rebuilt, "header", &[s("user-agent")])).text(), "Minecraft-PAPER");
        let buffer = ok(construct("Buffer", &[]));
        ok(method(&ok(method(&rebuilt, "body", &[])), "writeTo", &[buffer.clone()]));
        assert_eq!(ok(method(&buffer, "readString", &[s("UTF-8")])).text(), "{\"a\":1}");
        let charset = ok(method(&ok(method(&ok(method(&rebuilt, "body", &[])), "contentType", &[])), "charset", &[]));
        assert_eq!(charset.text(), "utf-8");
    }

    #[test]
    fn interceptors_run_in_order_and_can_answer() {
        // An app interceptor that answers by itself: OkHttp lets one return a response without proceeding.
        let answer = crate::func(|args: &[V]| {
            let chain = args[0].clone();
            let request = call(&chain, "request", &[])?;
            let b = construct("Response.Builder", &[]).unwrap()?;
            call(&b, "code", &[V::Int(418)])?;
            call(&b, "request", &[request])?;
            call(&b, "body", &[static_call("ResponseBody", "create", &[s("teapot")]).unwrap()?])?;
            call(&b, "build", &[])
        });
        let client = ok(construct("OkHttpClient", &[]));
        let builder = ok(method(&client, "newBuilder", &[]));
        ok(method(&builder, "addInterceptor", &[answer]));
        let client = ok(method(&builder, "build", &[]));
        let rb = ok(construct("Request.Builder", &[]));
        ok(method(&rb, "url", &[s("https://example.invalid/")]));
        let call_ = ok(method(&client, "newCall", &[ok(method(&rb, "build", &[]))]));
        let response = ok(method(&call_, "execute", &[]));
        assert_eq!(ok(method(&response, "code", &[])).int(), 418);
        assert!(!ok(method(&response, "isSuccessful", &[])).truth());
        assert_eq!(ok(method(&ok(method(&response, "body", &[])), "string", &[])).text(), "teapot");
    }

    #[test]
    fn a_host_that_doesnt_exist_is_an_ioexception() {
        let client = ok(construct("OkHttpClient", &[]));
        let rb = ok(construct("Request.Builder", &[]));
        ok(method(&rb, "url", &[s("https://no-such-host.invalid/")]));
        let call_ = ok(method(&client, "newCall", &[ok(method(&rb, "build", &[]))]));
        match method(&call_, "execute", &[]) {
            Some(Err(e)) => assert!(crate::instance_of(&e, "IOException") || e.text().contains("IOException")),
            _ => panic!("expected an IOException"),
        }
    }
}
