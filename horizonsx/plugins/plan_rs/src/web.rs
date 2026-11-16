//! The web server (Webserver.Port, default 8804) on threads of its own: a read-only analytics page made here
//! (overview, players online over time, retention, peak hours, top playtime, player search and a page per player)
//! and the JSON it reads (`/api/...`). It reads the database on its own connections and the online sessions from
//! memory: nothing here touches the game.

use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

static SERVER: Mutex<Option<(Arc<tiny_http::Server>, Vec<std::thread::JoinHandle<()>>)>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

pub fn start(bind: &str, port: u16, threads: usize) -> Result<(), String> {
    STOP.store(false, Ordering::Release);
    let addr = format!("{bind}:{port}");
    match tiny_http::Server::http(&addr) {
        Ok(s) => run(Arc::new(s), threads),
        Err(e) if e.to_string().contains("10048") || e.to_string().to_lowercase().contains("in use") => {
            // A hot reload: the old copy's listener closes on tiny_http's own thread a moment after its disable.
            // Bind again in the background for up to 10 s.
            let _ = std::thread::Builder::new().name("Plan web bind".into()).spawn(move || {
                for _ in 0..40 {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    if STOP.load(Ordering::Acquire) {
                        return;
                    }
                    if let Ok(s) = tiny_http::Server::http(&addr) {
                        run(Arc::new(s), threads);
                        return;
                    }
                }
            });
        }
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}

fn run(server: Arc<tiny_http::Server>, threads: usize) {
    let mut handles = Vec::new();
    for i in 0..threads.max(1) {
        let s = server.clone();
        if let Ok(h) = std::thread::Builder::new().name(format!("Plan web {i}")).spawn(move || serve(&s)) {
            handles.push(h);
        }
    }
    *SERVER.lock().unwrap_or_else(|e| e.into_inner()) = Some((server, handles));
}

pub fn stop() {
    STOP.store(true, Ordering::Release);
    if let Some((server, handles)) = SERVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        server.unblock();
        for _ in 1..handles.len() {
            server.unblock();
        }
        for h in handles {
            let _ = h.join();
        }
    }
}

fn serve(server: &tiny_http::Server) {
    while !STOP.load(Ordering::Acquire) {
        let Ok(req) = server.recv() else { return };
        if STOP.load(Ordering::Acquire) {
            return;
        }
        let url = req.url().to_owned();
        let (path, query) = url.split_once('?').map_or((url.as_str(), ""), |(p, q)| (p, q));
        let path = path.to_owned();
        let q = |key: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| decode(v))).unwrap_or_default();
        let now = crate::now();
        let offset = crate::tz_offset_ms();
        let (body, kind, status): (String, &str, u16) = match path.as_str() {
            "/" | "/server" | "/players" => (PAGE.to_owned(), "text/html; charset=utf-8", 200),
            p if p.starts_with("/player/") => (PAGE.to_owned(), "text/html; charset=utf-8", 200),
            "/api/overview" => {
                let mut v = crate::db::overview(now);
                v["online_now"] = json!(crate::live().len());
                v["server_name"] = json!(crate::server_name());
                v["now"] = json!(now);
                (v.to_string(), "application/json", 200)
            }
            "/api/online" => (crate::db::online(now, q("days").parse().unwrap_or(7).clamp(1, 365)).to_string(), "application/json", 200),
            "/api/retention" => (crate::db::retention(now).to_string(), "application/json", 200),
            "/api/peak_hours" => (crate::db::peak_hours(now, offset).to_string(), "application/json", 200),
            "/api/players" => (crate::db::players(&q("search"), q("limit").parse().unwrap_or(250).clamp(1, 25000)).to_string(), "application/json", 200),
            "/api/live" => (Value::Array(crate::live().into_iter().map(|(uuid, name, start, afk)| json!({ "uuid": uuid, "name": name, "start": start, "afk": afk })).collect()).to_string(), "application/json", 200),
            p if p.starts_with("/api/player/") => {
                let who = decode(&p["/api/player/".len()..]);
                let mut v = crate::db::player(&who);
                if v.is_null() {
                    (json!({ "error": "not found" }).to_string(), "application/json", 404)
                } else {
                    if let Some((_, _, start, afk)) = crate::live().into_iter().find(|(u, n, _, _)| v["uuid"] == *u || n.eq_ignore_ascii_case(&who)) {
                        v["online_since"] = json!(start);
                        v["online_afk"] = json!(afk);
                    }
                    (v.to_string(), "application/json", 200)
                }
            }
            _ => ("Not found".to_owned(), "text/plain", 404),
        };
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], kind.as_bytes()).expect("header");
        let cache = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).expect("header");
        let _ = req.respond(tiny_http::Response::from_string(body).with_header(header).with_header(cache).with_status_code(status));
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    out.push(b);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const PAGE: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Plan</title>
<style>
:root{--bg:#f6f7f9;--card:#fff;--ink:#1d2330;--mute:#6b7385;--line:#e3e6ec;--acc:#2f7d5b;--acc2:#3a6ea5;--warn:#c0573e}
@media (prefers-color-scheme:dark){:root{--bg:#14171c;--card:#1c2027;--ink:#e6e9ef;--mute:#9aa3b2;--line:#2b313b;--acc:#4fb486;--acc2:#6c9ed6;--warn:#e07a5f}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.45 system-ui,-apple-system,Segoe UI,Roboto,sans-serif}
header{display:flex;gap:16px;align-items:center;padding:14px 20px;border-bottom:1px solid var(--line);background:var(--card);flex-wrap:wrap}
header h1{font-size:18px;margin:0 12px 0 0}header a{color:var(--mute);text-decoration:none;padding:4px 2px}header a.on{color:var(--ink);border-bottom:2px solid var(--acc)}
main{max-width:1180px;margin:0 auto;padding:18px 16px 40px}
.grid{display:grid;gap:14px;grid-template-columns:repeat(auto-fit,minmax(170px,1fr))}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:14px 16px}
.card h2{font-size:13px;font-weight:600;color:var(--mute);margin:0 0 6px;text-transform:uppercase;letter-spacing:.04em}
.big{font-size:24px;font-weight:650}.sub{color:var(--mute);font-size:13px}
.wide{margin-top:14px}.two{display:grid;gap:14px;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));margin-top:14px}
svg{width:100%;height:auto;display:block}svg text{fill:var(--mute);font-size:11px}
table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:7px 6px;border-bottom:1px solid var(--line)}th{color:var(--mute);font-weight:600;font-size:13px}
td a{color:var(--acc2);text-decoration:none}input{font:inherit;padding:7px 10px;border:1px solid var(--line);border-radius:6px;background:var(--bg);color:var(--ink);width:260px;max-width:100%}
.dot{display:inline-block;width:8px;height:8px;border-radius:50%;background:var(--acc);margin-right:6px}.right{text-align:right}
</style></head><body>
<header><h1 id="sname">Plan</h1><a href="/" data-v="server">Server</a><a href="/players" data-v="players">Players</a><span class="sub" id="stamp"></span></header>
<main id="app"></main>
<script>
const $=s=>document.querySelector(s);const app=$('#app');
const fmt=ms=>{if(!ms||ms<0)return'0s';let s=Math.floor(ms/1000),d=Math.floor(s/86400),h=Math.floor(s%86400/3600),m=Math.floor(s%3600/60);return d?`${d}d ${h}h`:h?`${h}h ${m}m`:m?`${m}m ${s%60}s`:`${s}s`};
const date=t=>t?new Date(t).toLocaleString():'-';const day=t=>t?new Date(t).toLocaleDateString():'-';
const esc=s=>String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const get=u=>fetch(u).then(r=>r.json());
function card(t,v,s){return `<div class="card"><h2>${t}</h2><div class="big">${v}</div>${s?`<div class="sub">${s}</div>`:''}</div>`}
function line(pts,{h=200,color='var(--acc)',y0=0,label=''}={}){const W=900,H=h,P=34;if(!pts.length)return'<div class="sub">No data yet: points arrive once a minute.</div>';
 const xs=pts.map(p=>p[0]),ys=pts.map(p=>p[1]);const x0=Math.min(...xs),x1=Math.max(...xs)||x0+1,ym=Math.max(y0+1,...ys);
 const X=x=>P+(x-x0)/(x1-x0||1)*(W-P-8),Y=y=>H-20-(y/ym)*(H-30);let d=pts.map((p,i)=>(i?'L':'M')+X(p[0]).toFixed(1)+','+Y(p[1]).toFixed(1)).join('');
 let g='';for(let i=0;i<=4;i++){const v=ym*i/4,yy=Y(v);g+=`<line x1="${P}" x2="${W}" y1="${yy}" y2="${yy}" stroke="var(--line)"/><text x="2" y="${yy+4}">${Math.round(v*10)/10}</text>`}
 g+=`<text x="${P}" y="${H-4}">${day(x0)}</text><text x="${W-80}" y="${H-4}">${day(x1)}</text>`;
 return `<svg viewBox="0 0 ${W} ${H}">${g}<path d="${d}" fill="none" stroke="${color}" stroke-width="1.6"/></svg>${label?`<div class="sub">${label}</div>`:''}`}
function bars(vals,{h=170,color='var(--acc2)',labels=null}={}){const W=900,H=h,P=30,n=vals.length,mx=Math.max(1,...vals),bw=(W-P)/n;
 return `<svg viewBox="0 0 ${W} ${H}">`+vals.map((v,i)=>{const bh=v/mx*(H-34);return `<rect x="${P+i*bw+2}" y="${H-18-bh}" width="${bw-4}" height="${bh}" fill="${color}" rx="2"/><text x="${P+i*bw+bw/2-6}" y="${H-4}">${labels?labels[i]:i}</text>`}).join('')+`<text x="2" y="14">${Math.round(mx*10)/10}</text></svg>`}
async function server(){app.innerHTML='<div class="sub">Loading…</div>';
 const [o,on,ret,ph,top,live]=await Promise.all([get('/api/overview'),get('/api/online?days=7'),get('/api/retention'),get('/api/peak_hours'),get('/api/players?limit=10'),get('/api/live')]);
 $('#sname').textContent=o.server_name||'Plan';$('#stamp').textContent='as of '+date(o.now);
 app.innerHTML=`<div class="grid">${card('Online now',o.online_now,'peak '+o.peak_2d+' in 2 days, '+o.peak_all+' all time')}${card('Players',o.players,'+'+o.new_day+' today, +'+o.new_week+' this week')}
 ${card('Unique players',o.unique_week,o.unique_day+' today, '+o.unique_month+' in 30 days')}${card('Playtime',fmt(o.playtime),fmt(o.afk)+' of it AFK')}
 ${card('Sessions',o.sessions,'average '+fmt(o.avg_session))}${card('TPS (7 days)',(o.tps_week||0).toFixed(2),o.low_tps_minutes_week+' minutes under 10')}
 ${card('Kills',o.player_kills,o.mob_kills+' mobs, '+o.deaths+' deaths')}</div>
 <div class="card wide"><h2>Players online, last 7 days</h2>${line(on.map(r=>[r[0],r[1]||0]))}</div>
 <div class="card wide"><h2>TPS, last 7 days</h2>${line(on.map(r=>[r[0],r[2]||0]),{color:'var(--warn)',h:150})}</div>
 <div class="two"><div class="card"><h2>Peak hours: sessions started (30 days)</h2>${bars(ph.session_starts||[])}</div>
 <div class="card"><h2>Peak hours: average online (30 days)</h2>${bars(ph.average_online||[],{color:'var(--acc)'})}</div></div>
 <div class="two"><div class="card"><h2>Retention by join week</h2><table><tr><th>Week of</th><th class="right">Joined</th><th class="right">Back after a day</th><th class="right">Back after a week</th></tr>
 ${(ret||[]).map(r=>`<tr><td>${day(r.from)}</td><td class="right">${r.joined}</td><td class="right">${r.joined?Math.round(r.returned_day/r.joined*100)+'%':'-'}</td><td class="right">${r.joined?Math.round(r.returned_week/r.joined*100)+'%':'-'}</td></tr>`).join('')}</table></div>
 <div class="card"><h2>Top playtime</h2><table><tr><th>Player</th><th class="right">Playtime</th><th class="right">Sessions</th></tr>
 ${(top||[]).map(p=>`<tr><td><a href="/player/${encodeURIComponent(p.name)}">${esc(p.name)}</a></td><td class="right">${fmt(p.playtime)}</td><td class="right">${p.sessions}</td></tr>`).join('')}</table></div></div>
 <div class="card wide"><h2>Online now</h2>${live.length?`<table><tr><th>Player</th><th>Since</th><th class="right">Session</th></tr>${live.map(p=>`<tr><td><span class="dot"></span><a href="/player/${encodeURIComponent(p.name)}">${esc(p.name)}</a></td><td>${date(p.start)}</td><td class="right">${fmt(o.now-p.start)}</td></tr>`).join('')}</table>`:'<div class="sub">Nobody is online.</div>'}</div>`}
async function players(){app.innerHTML=`<div class="card"><h2>Players</h2><input id="q" placeholder="Search a name"><div id="list" style="margin-top:10px"></div></div>`;
 const load=async()=>{const rows=await get('/api/players?limit=2500&search='+encodeURIComponent($('#q').value));
 $('#list').innerHTML=`<table><tr><th>Name</th><th>Registered</th><th>Last seen</th><th class="right">Playtime</th><th class="right">Sessions</th></tr>${rows.map(p=>`<tr><td><a href="/player/${encodeURIComponent(p.name)}">${esc(p.name)}</a></td><td>${day(p.registered)}</td><td>${date(p.last_seen)}</td><td class="right">${fmt(p.playtime)}</td><td class="right">${p.sessions}</td></tr>`).join('')}</table>`};
 let t;$('#q').oninput=()=>{clearTimeout(t);t=setTimeout(load,250)};load()}
async function player(name){const p=await get('/api/player/'+encodeURIComponent(name));if(p.error){app.innerHTML='<div class="card">No such player.</div>';return}
 const wt=p.worlds||[];const tot=wt.reduce((a,w)=>a+w.survival+w.creative+w.adventure+w.spectator,0)||1;
 app.innerHTML=`<div class="card"><h2>Player</h2><div class="big">${p.online_since?'<span class="dot"></span>':''}${esc(p.name)}</div><div class="sub">${p.uuid}${p.nicknames&&p.nicknames.length>1?' · also '+p.nicknames.filter(n=>n!==p.name).map(esc).join(', '):''}</div></div>
 <div class="grid wide">${card('Playtime',fmt(p.playtime),fmt(p.afk)+' AFK, '+fmt(p.playtime-p.afk)+' active')}${card('Sessions',p.sessions_count,p.sessions_count?'average '+fmt(p.playtime/p.sessions_count):'')}
 ${card('Registered',day(p.registered),'last seen '+(p.online_since?'now':date(p.last_seen)))}${card('Kills',p.player_kills,p.mob_kills+' mobs, '+p.deaths+' deaths')}
 ${card('Ping',p.ping&&p.ping.avg!=null?Math.round(p.ping.avg)+' ms':'-',p.ping&&p.ping.min!=null?p.ping.min+'-'+p.ping.max+' ms':'')}${card('Kicked',p.times_kicked,'')}</div>
 <div class="two"><div class="card"><h2>World times</h2><table><tr><th>World</th><th class="right">Time</th><th class="right">Share</th></tr>${wt.map(w=>{const t=w.survival+w.creative+w.adventure+w.spectator;return `<tr><td>${esc(w.world)}</td><td class="right">${fmt(t)}</td><td class="right">${Math.round(t/tot*100)}%</td></tr>`}).join('')}</table></div>
 <div class="card"><h2>Recent kills</h2>${p.kills.length?`<table><tr><th>When</th><th>Victim</th><th>Weapon</th></tr>${p.kills.map(k=>`<tr><td>${date(k.date)}</td><td>${esc(k.victim)}</td><td>${esc(k.weapon)}</td></tr>`).join('')}</table>`:'<div class="sub">None.</div>'}</div></div>
 <div class="card wide"><h2>Sessions</h2><table><tr><th>Start</th><th class="right">Length</th><th class="right">AFK</th><th class="right">Mob kills</th><th class="right">Deaths</th></tr>
 ${p.online_since?`<tr><td><span class="dot"></span>${date(p.online_since)}</td><td class="right">${fmt(Date.now()-p.online_since)}</td><td class="right">${fmt(p.online_afk)}</td><td class="right">-</td><td class="right">-</td></tr>`:''}
 ${p.sessions.map(s=>`<tr><td>${date(s.start)}</td><td class="right">${fmt(s.end-s.start)}</td><td class="right">${fmt(s.afk)}</td><td class="right">${s.mob_kills}</td><td class="right">${s.deaths}</td></tr>`).join('')}</table></div>`}
function route(){const p=location.pathname;document.querySelectorAll('header a').forEach(a=>a.classList.toggle('on',(a.dataset.v==='players'&&p.startsWith('/player'))||(a.dataset.v==='server'&&(p==='/'||p==='/server'))));
 if(p.startsWith('/player/'))player(decodeURIComponent(p.slice(8)));else if(p==='/players')players();else server()}
route();
</script></body></html>
"##;
