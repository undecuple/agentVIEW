//! Request in, response out. GET only. No network, no writes, no approvals.
use crate::{gate, html::escape, images, policy};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use similar::TextDiff;
use std::path::Path;
use std::sync::OnceLock;

pub use policy::Config;
use policy::Refused;

const MAX_BYTES: usize = 1_000_000;
const GATE_BYTES: usize = 16_000_000;
const DIFF_BYTES: usize = 200_000;
const DIFF_LINES: usize = 2000;
const MAX_REF_LINES: usize = 5000;

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

// Keep '/' and '.' literal in our own links; encode the rest.
const QUOTE: &AsciiSet = &CONTROLS
    .add(b' ').add(b'"').add(b'#').add(b'%').add(b'&').add(b'+').add(b'<').add(b'>').add(b'?')
    .add(b'`').add(b'{').add(b'}').add(b'\\').add(b'^').add(b'|').add(b'[').add(b']').add(b'\'');

fn q(s: &str) -> String {
    utf8_percent_encode(s, QUOTE).to_string()
}

const PAGE_HEAD: &str = "<!doctype html><meta charset=utf-8><title>agent-app</title>\n<style>body{font:15px system-ui;max-width:60rem;margin:1rem auto;padding:0 1rem}\nnav a{margin-right:1rem}pre{white-space:pre-wrap;background:#8881;padding:.5rem}\n.add{color:#0a0}.del{color:#c00}</style>\n<nav><a href=\"/\">Docs</a><a href=\"/gate\">Gate status</a><a href=\"/diff\">Diff</a></nav>\n<p><i>Read-only. This page never grants an approval; approval is given by a person, elsewhere.</i></p>\n";

fn common(mut h: Vec<(String, String)>) -> Vec<(String, String)> {
    for (k, v) in [
        ("X-Content-Type-Options", "nosniff"),
        ("Cache-Control", "no-store"),
        ("Referrer-Policy", "no-referrer"),
        ("Cross-Origin-Resource-Policy", "same-origin"),
    ] {
        h.push((k.into(), v.into()));
    }
    h
}

fn page(body: &str, status: u16) -> Response {
    let headers = common(vec![
        ("Content-Type".into(), "text/html; charset=utf-8".into()),
        (
            "Content-Security-Policy".into(),
            "default-src 'none'; style-src 'unsafe-inline'; img-src 'self'; frame-ancestors 'none'; form-action 'none'; base-uri 'none'".into(),
        ),
    ]);
    Response { status, headers, body: format!("{PAGE_HEAD}{body}").into_bytes() }
}

/// Query parsing: decoded exactly once. Returns Err for anything the design refuses.
fn parse_query(query: &str) -> Result<Vec<(String, String)>, Refused> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let lv = v.to_ascii_lowercase();
        if lv.contains("%00") || lv.contains("%2e") || lv.contains("%2f") {
            return Err(Refused("encoded dot, slash or NUL refused".into()));
        }
        let dec = percent_decode_str(&v.replace('+', " ")).decode_utf8().map_err(|_| Refused("invalid UTF-8".into()))?.into_owned();
        if dec.contains('\0') || dec.split('/').any(|c| c == "..") {
            return Err(Refused("bad path".into()));
        }
        if !seen.insert(k.to_string()) {
            return Err(Refused("repeated parameter".into()));
        }
        out.push((k.to_string(), dec));
    }
    Ok(out)
}

fn get<'a>(qs: &'a [(String, String)], k: &str) -> &'a str {
    qs.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str()).unwrap_or("")
}

fn ref_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"!\[[^\]]{0,200}\]\(([^)]{1,300})\)").unwrap())
}

fn doc_images(real: &Path, text: &str, cfg: &Config) -> String {
    let mut out: Vec<String> = Vec::new();
    'lines: for line in gate::splitlines(text).take(MAX_REF_LINES) {
        let cut: String = line.chars().take(2000).collect();
        for m in ref_re().captures_iter(&cut) {
            if out.len() >= 20 {
                break;
            }
            let raw = m[1].trim();
            let r = raw.split_whitespace().next().unwrap_or("");
            let shown = escape(&policy::redact(&r.chars().take(80).collect::<String>()));
            if r.is_empty() || r.contains("://") || r.starts_with(['/', '~']) || r.starts_with("data:") {
                out.push(format!("<li>not shown (only relative local images): {shown}</li>"));
                continue;
            }
            let target = real.parent().unwrap_or(Path::new("/")).join(r);
            let t = target.to_string_lossy().into_owned();
            let ok = images::is_image_ext(&t) && policy::check(&t, cfg).is_ok();
            match (ok, policy::check(&t, cfg)) {
                (true, Ok(rp)) => out.push(format!(
                    "<li><img src=\"/img?p={}\" alt=\"\" style=\"max-width:100%\"></li>",
                    q(&rp.to_string_lossy())
                )),
                _ => out.push(format!("<li>not shown: {shown}</li>")),
            }
        }
        if out.len() >= 20 {
            out.push("<li>more references not processed (limit 20)</li>".into());
            break 'lines;
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("<h3>Images referenced</h3><ul>{}</ul>", out.join(""))
    }
}

fn send_image(p: &str, cfg: &Config, sec_fetch_site: Option<&str>) -> Result<Response, Refused> {
    if let Some(s) = sec_fetch_site {
        if s != "same-origin" && s != "none" {
            return Err(Refused("cross-site request".into()));
        }
    }
    let (real, data, more) = policy::read_bytes(p, cfg, images::MAX_IMG_BYTES)?;
    if more {
        return Err(Refused("file too large".into()));
    }
    let mime = images::validate(&data, &real.to_string_lossy()).map_err(|e| Refused(e.0.into()))?;
    let headers = common(vec![
        ("Content-Type".into(), mime.into()),
        ("Content-Security-Policy".into(), "default-src 'none'; sandbox; frame-ancestors 'none'; base-uri 'none'; form-action 'none'".into()),
    ]);
    Ok(Response { status: 200, headers, body: data })
}

fn short_err(_r: &Refused) -> String {
    // One message for every cause: no oracle for what exists, and no resolved target names.
    "path not available".to_string()
}

/// Handle one GET. `target` is the request path plus query. Anything else gives 404 or 405.
pub fn handle(cfg: &Config, method: &str, target: &str, sec_fetch_site: Option<&str>) -> Response {
    if method != "GET" {
        return page("read-only", 405);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match route(cfg, path, query, sec_fetch_site) {
        Ok(r) => r,
        Err(e) => page(&format!("<p>Refused: {}</p>", short_err(&e)), 403),
    }
}

fn route(cfg: &Config, path: &str, query: &str, sfs: Option<&str>) -> Result<Response, Refused> {
    let qs = parse_query(query)?;
    match path {
        "/" => {
            let items: String = policy::listing(cfg)
                .iter()
                .map(|f| {
                    let s = f.to_string_lossy();
                    format!("<li><a href=\"/view?p={}\">{}</a></li>", q(&s), escape(&policy::short(&s)))
                })
                .collect();
            Ok(page(&format!("<h2>Allow-listed docs</h2><ul>{items}</ul>"), 200))
        }
        "/img" => send_image(get(&qs, "p"), cfg, sfs),
        "/view" => {
            let p = get(&qs, "p");
            let real = policy::check(p, cfg)?;
            let rs = real.to_string_lossy().into_owned();
            if images::is_image_ext(&rs) {
                return Ok(page(
                    &format!(
                        "<h2>{}</h2><img src=\"/img?p={}\" alt=\"\" style=\"max-width:100%\">",
                        escape(&policy::short(&rs)),
                        q(&rs)
                    ),
                    200,
                ));
            }
            let (real, text, more) = policy::read_text(p, cfg, MAX_BYTES)?;
            let note = if more { format!("<p><b>Truncated: showing the first {MAX_BYTES} bytes.</b></p>") } else { String::new() };
            Ok(page(
                &format!(
                    "<h2>{}</h2>{note}<pre>{}</pre>{}",
                    escape(&policy::short(&real.to_string_lossy())),
                    escape(&policy::redact(&text)),
                    doc_images(&real, &text, cfg)
                ),
                200,
            ))
        }
        "/gate" => {
            let Some(g) = &cfg.gate_file else { return Ok(page("<p>No gate file configured.</p>", 200)) };
            let (_, text, more) = policy::read_text(&g.to_string_lossy(), cfg, GATE_BYTES)?;
            let note = if more {
                "<p><b>Truncated: the file is larger than the read limit, older rows are missing.</b></p>"
            } else {
                ""
            };
            let rows: String =
                gate::pending(&text, 50).iter().map(|r| format!("<li>{}</li>", escape(&policy::redact(r)))).collect();
            Ok(page(
                &format!(
                    "<h2>Possibly pending (best effort)</h2><p>Best-effort text parse; rows must be newest first. A PASS row is listed when no later row names the same head with a closing word. A PASS row you have just approved stays listed until a separate later row (the read-back) names the same head. An empty list does NOT mean nothing is pending; check the source.</p>{note}<ul>{rows}</ul>"
                ),
                200,
            ))
        }
        "/diff" => {
            let (a, b) = (get(&qs, "a"), get(&qs, "b"));
            if a.is_empty() || b.is_empty() {
                return Ok(page("<p>Use /diff?a=PATH&amp;b=PATH, both allow-listed.</p>", 200));
            }
            let (ra, ta, t1) = policy::read_text(a, cfg, DIFF_BYTES)?;
            let (rb, tb, t2) = policy::read_text(b, cfg, DIFF_BYTES)?;
            let mut lines: Vec<String> = Vec::new();
            if t1 || t2 {
                lines.push(escape(&format!("[inputs truncated to {DIFF_BYTES} bytes each]")));
            }
            let (ta, tb) = (policy::redact(&ta), policy::redact(&tb));
            let diff = TextDiff::configure()
                .algorithm(similar::Algorithm::Myers)
                .timeout(std::time::Duration::from_secs(2))
                .diff_lines(&ta, &tb);
            let out = diff
                .unified_diff()
                .context_radius(3)
                .header(&policy::short(&ra.to_string_lossy()), &policy::short(&rb.to_string_lossy()))
                .to_string();
            for l in out.lines() {
                if lines.len() >= DIFF_LINES {
                    lines.push(escape("[output truncated]"));
                    break;
                }
                let c = if l.starts_with('+') { "add" } else if l.starts_with('-') { "del" } else { "" };
                lines.push(format!("<span class=\"{c}\">{}</span>", escape(l)));
            }
            Ok(page(&format!("<pre>{}</pre>", lines.join("\n")), 200))
        }
        _ => Ok(page("not found", 404)),
    }
}
