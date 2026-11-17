//! Test-only (feature "e2e"): a page that probes what the webview is allowed to do, and a report sink.
use tauri::http::Response;

const PAGE: &str = r#"<!doctype html><meta charset=utf-8><title>e2e</title>
<a id=l target=_blank href="https://example.com/link">x</a>
<img src="/img?p=/nonexistent.png">
<script>
const r = {};
const rep = (k, v) => { new Image().src = '/__report?' + encodeURIComponent(k) + '=' + encodeURIComponent(String(v)); };
(async () => {
  rep('tauri_internals', typeof window.__TAURI_INTERNALS__);
  rep('tauri_global', typeof window.__TAURI__);
  try {
    if (window.__TAURI_INTERNALS__) {
      try { await window.__TAURI_INTERNALS__.invoke('anything'); rep('invoke', 'RESOLVED'); }
      catch (e) { rep('invoke', 'rejected'); }
    } else { rep('invoke', 'no-internals'); }
  } catch (e) { rep('invoke', 'threw'); }
  for (const u of ['https://example.com/', 'file:///etc/passwd', 'viewer://localhost/']) {
    let w = null; try { w = window.open(u, '_blank'); } catch (e) {}
    rep('open ' + u, w ? 'returned-window' : 'null');
  }
  document.getElementById('l').click();
  setTimeout(() => { rep('before_nav', 'ok'); location.href = 'https://example.com/'; }, 800);
  setTimeout(() => { rep('still_here_after_nav', 'yes'); }, 2500);
  setTimeout(() => { rep('done', '1'); }, 3000);
})();
</script>"#;

pub fn intercept(target: &str, sfs: Option<&str>) -> Option<Response<Vec<u8>>> {
    if target.starts_with("/img?") {
        eprintln!("E2E sec-fetch-site={:?}", sfs);
    }
    if target == "/__e2e" {
        return Some(
            Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Content-Security-Policy", "default-src 'none'; script-src 'unsafe-inline'; img-src 'self'")
                .body(PAGE.as_bytes().to_vec())
                .unwrap(),
        );
    }
    if let Some(q) = target.strip_prefix("/__report?") {
        eprintln!("E2E report {q}");
        if q.starts_with("done=") {
            std::process::exit(0);
        }
        return Some(Response::builder().status(204).body(Vec::new()).unwrap());
    }
    None
}
