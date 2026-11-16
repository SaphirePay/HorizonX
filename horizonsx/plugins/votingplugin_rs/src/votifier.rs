//! The Votifier receiver, on threads of its own: NuVotifier's protocol on one TCP port. The greeting
//! `VOTIFIER 2 <challenge>`, then either a v2 message (0x733A, length, JSON `{payload, signature}` signed with
//! HMAC-SHA256 by the service's token, the payload carrying the challenge) answered with `{"status":"ok"}`, or a
//! v1 block (256 bytes, RSA PKCS#1 v1.5, `VOTE\nservice\nuser\naddress\ntime\n`) decrypted with the private key.
//! Keys: plugins/Votifier/rsa (NuVotifier's, base64 DER) when present, else rsa/ in VotingPlugin's folder, made
//! on first start.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static STOP: AtomicBool = AtomicBool::new(false);
static ACCEPTOR: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static OPEN: AtomicUsize = AtomicUsize::new(0);

struct Keys {
    private: Option<RsaPrivateKey>,
    tokens: HashMap<String, String>,
    debug: bool,
}

fn rsa_dir(folder: &Path) -> PathBuf {
    let nu = folder.parent().map(|p| p.join("Votifier").join("rsa"));
    match nu {
        Some(d) if d.join("private.key").exists() => d,
        _ => folder.join("rsa"),
    }
}

fn read_b64(path: &Path) -> Option<Vec<u8>> {
    let text = std::fs::read_to_string(path).ok()?;
    let clean: String = text.lines().filter(|l| !l.starts_with("-----")).collect::<String>().chars().filter(|c| !c.is_whitespace()).collect();
    B64.decode(clean).ok()
}

/// The private key (made and written when there is none).
fn load_key(folder: &Path) -> Option<RsaPrivateKey> {
    let dir = rsa_dir(folder);
    if let Some(der) = read_b64(&dir.join("private.key"))
        && let Ok(k) = RsaPrivateKey::from_pkcs8_der(&der)
    {
        return Some(k);
    }
    let mut rng = rand::thread_rng();
    let key = RsaPrivateKey::new(&mut rng, 2048).ok()?;
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(der) = key.to_pkcs8_der() {
        let _ = std::fs::write(dir.join("private.key"), B64.encode(der.as_bytes()));
    }
    if let Ok(der) = RsaPublicKey::from(&key).to_public_key_der() {
        let _ = std::fs::write(dir.join("public.key"), B64.encode(der.as_bytes()));
    }
    crate::log(&format!("Votifier: made a new RSA key pair in {}", dir.display()));
    Some(key)
}

fn random_token() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..26).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

/// Starts the listener (VotingPlugin's Votifier.yml: Enabled, Host, Port, Tokens).
pub fn start(folder: PathBuf) {
    let s = crate::settings::get();
    let c = &s.votifier;
    if !c.bool("Enabled", true) {
        return;
    }
    let host = c.str("Host", "0.0.0.0");
    let port = c.int("Port", 8192);
    let debug = c.bool("Debug", false);
    let mut tokens: HashMap<String, String> = HashMap::new();
    for k in c.keys("Tokens") {
        let v = c.str(&format!("Tokens.{k}"), "");
        if !v.is_empty() {
            tokens.insert(k.to_ascii_lowercase(), v);
        }
    }
    // NuVotifier's own tokens, if its config is there.
    if let Some(nu) = folder.parent().map(|p| p.join("Votifier").join("config.yml")).filter(|p| p.exists()) {
        let n = horizon_kit::Config::load(nu.parent().unwrap_or(Path::new(".")), "config.yml", "");
        for k in n.keys("tokens") {
            let v = n.str(&format!("tokens.{k}"), "");
            if !v.is_empty() {
                tokens.entry(k.to_ascii_lowercase()).or_insert(v);
            }
        }
    }
    if !tokens.contains_key("default") {
        let t = random_token();
        let mut cfg = c.clone();
        cfg.set("Tokens.default", serde_yaml::Value::String(t.clone()));
        cfg.save();
        crate::log("Votifier: made a v2 token (Votifier.yml, Tokens.default).");
        tokens.insert("default".into(), t);
    }
    STOP.store(false, Ordering::SeqCst);
    let handle = std::thread::Builder::new()
        .name("VotingPlugin-Votifier".into())
        .spawn(move || {
            let keys = Arc::new(Keys { private: load_key(&folder), tokens, debug });
            let listener = match TcpListener::bind(format!("{host}:{port}")) {
                Ok(l) => l,
                Err(e) => {
                    crate::log(&format!("Votifier: could not listen on {host}:{port}: {e}"));
                    return;
                }
            };
            let _ = listener.set_nonblocking(true);
            crate::log(&format!("Votifier: listening on {host}:{port} (v1 RSA + v2 tokens)."));
            while !STOP.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        if OPEN.load(Ordering::SeqCst) > 64 {
                            continue;
                        }
                        OPEN.fetch_add(1, Ordering::SeqCst);
                        let keys = keys.clone();
                        std::thread::spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            if let Err(e) = handle_conn(stream, &keys)
                                && keys.debug
                            {
                                crate::log(&format!("Votifier: {addr}: {e}"));
                            }
                            OPEN.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
        })
        .ok();
    *ACCEPTOR.lock().unwrap_or_else(|e| e.into_inner()) = handle;
}

pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
    if let Some(h) = ACCEPTOR.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
}

fn handle_conn(mut stream: TcpStream, keys: &Keys) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let challenge = random_token();
    stream.write_all(format!("VOTIFIER 2 {challenge}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).map_err(|e| e.to_string())?;
    if head == [0x73, 0x3A] {
        let mut len = [0u8; 2];
        stream.read_exact(&mut len).map_err(|e| e.to_string())?;
        let n = u16::from_be_bytes(len) as usize;
        let mut buf = vec![0u8; n];
        stream.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let reply = match v2(&buf, &challenge, keys) {
            Ok(()) => serde_json::json!({ "status": "ok" }),
            Err(e) => serde_json::json!({ "status": "error", "cause": "CorruptedVoteException", "error": e }),
        };
        let _ = stream.write_all(format!("{reply}\r\n").as_bytes());
        return Ok(());
    }
    // v1: the rest of a 256-byte RSA block.
    let mut block = vec![0u8; 256];
    block[..2].copy_from_slice(&head);
    stream.read_exact(&mut block[2..]).map_err(|e| e.to_string())?;
    let key = keys.private.as_ref().ok_or("no RSA key")?;
    let plain = key.decrypt(Pkcs1v15Encrypt, &block).map_err(|_| "could not decrypt the v1 vote (wrong public key at the vote site?)".to_owned())?;
    let text = String::from_utf8_lossy(&plain).to_string();
    let mut lines = text.split('\n');
    if lines.next() != Some("VOTE") {
        return Err("v1 block without VOTE".into());
    }
    let service = lines.next().unwrap_or_default().to_owned();
    let user = lines.next().unwrap_or_default().to_owned();
    let address = lines.next().unwrap_or_default().to_owned();
    let time = lines.next().unwrap_or_default().trim().to_owned();
    crate::votes::received(&service, &user, &address, &time);
    Ok(())
}

fn v2(buf: &[u8], challenge: &str, keys: &Keys) -> Result<(), String> {
    let msg: serde_json::Value = serde_json::from_slice(buf).map_err(|_| "bad JSON".to_owned())?;
    let payload_text = msg["payload"].as_str().ok_or("no payload")?;
    let signature = B64.decode(msg["signature"].as_str().ok_or("no signature")?).map_err(|_| "bad signature".to_owned())?;
    let payload: serde_json::Value = serde_json::from_str(payload_text).map_err(|_| "bad payload".to_owned())?;
    let service = payload["serviceName"].as_str().unwrap_or_default().to_owned();
    let token = keys.tokens.get(&service.to_ascii_lowercase()).or_else(|| keys.tokens.get("default")).ok_or("unknown service")?;
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).map_err(|_| "bad token".to_owned())?;
    mac.update(payload_text.as_bytes());
    mac.verify_slice(&signature).map_err(|_| "signature does not match".to_owned())?;
    if payload["challenge"].as_str() != Some(challenge) {
        return Err("challenge does not match".into());
    }
    let user = payload["username"].as_str().unwrap_or_default().to_owned();
    let address = payload["address"].as_str().unwrap_or_default().to_owned();
    let time = payload["timestamp"].as_i64().map(|t| t.to_string()).or_else(|| payload["timestamp"].as_str().map(str::to_owned)).unwrap_or_default();
    crate::votes::received(&service, &user, &address, &time);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    fn serve(keys: Keys) -> (std::net::SocketAddr, std::thread::JoinHandle<Result<(), String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            handle_conn(s, &keys)
        });
        (addr, h)
    }

    #[test]
    fn v2_token_vote() {
        let mut tokens = HashMap::new();
        tokens.insert("default".to_owned(), "secret".to_owned());
        let (addr, h) = serve(Keys { private: None, tokens, debug: false });
        let mut c = TcpStream::connect(addr).unwrap();
        let mut greet = String::new();
        std::io::BufReader::new(c.try_clone().unwrap()).read_line(&mut greet).unwrap();
        let challenge = greet.trim().rsplit(' ').next().unwrap().to_owned();
        let payload = serde_json::json!({"serviceName": "TestSite", "username": "Steve", "address": "1.2.3.4", "timestamp": 1, "challenge": challenge}).to_string();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(payload.as_bytes());
        let sig = B64.encode(mac.finalize().into_bytes());
        let msg = serde_json::json!({"payload": payload, "signature": sig}).to_string();
        let mut out = vec![0x73, 0x3A];
        out.extend_from_slice(&(msg.len() as u16).to_be_bytes());
        out.extend_from_slice(msg.as_bytes());
        c.write_all(&out).unwrap();
        let mut reply = String::new();
        std::io::BufReader::new(c).read_line(&mut reply).unwrap();
        assert!(reply.contains("\"ok\""), "{reply}");
        h.join().unwrap().unwrap();
    }

    #[test]
    fn v1_rsa_vote() {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = RsaPublicKey::from(&key);
        let (addr, h) = serve(Keys { private: Some(key), tokens: HashMap::new(), debug: false });
        let mut c = TcpStream::connect(addr).unwrap();
        let block = public.encrypt(&mut rng, Pkcs1v15Encrypt, b"VOTE\nTestSite\nSteve\n1.2.3.4\n1\n").unwrap();
        assert_eq!(block.len(), 256);
        c.write_all(&block).unwrap();
        h.join().unwrap().unwrap();
    }
}
