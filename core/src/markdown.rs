//! Small, safe Markdown renderer for reviewing drafts. All text is HTML-escaped first; nothing in the
//! source can produce a script, an element with an attribute, an image or a live link. Links are shown as
//! text with their address visible. Supported: headings, paragraphs, lists, fenced code, quotes, rules,
//! simple tables, `code`, **bold** and *italic*.
use crate::html::escape;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

struct Rx { bold: Regex, italic: Regex, image: Regex, link: Regex }
fn rx() -> &'static Rx {
    static R: OnceLock<Rx> = OnceLock::new();
    R.get_or_init(|| Rx {
        bold: Regex::new(r"\*\*([^*\n]+)\*\*").unwrap(),
        italic: Regex::new(r"\*([^*\s][^*\n]*)\*").unwrap(),
        image: Regex::new(r"!\[([^\]\n]*)\]\(([^)\s]*)\)").unwrap(),
        link: Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)").unwrap(),
    })
}

fn inline_plain(s: &str) -> String {
    let r = rx();
    let t = escape(s);
    let t = r.image.replace_all(&t, "[image: $1]").into_owned();
    let t = r.link.replace_all(&t, "$1 <small class=\"md-url\">($2)</small>").into_owned();
    let t = r.bold.replace_all(&t, "<strong>$1</strong>").into_owned();
    r.italic.replace_all(&t, "<em>$1</em>").into_owned()
}

fn inline(s: &str) -> String {
    s.split('`').enumerate().map(|(i, part)| {
        if i % 2 == 1 { format!("<code>{}</code>", escape(part)) } else { inline_plain(part) }
    }).collect::<Vec<_>>().join("")
        // An unmatched backtick leaves an odd number of parts; the last part is then plain text, never code.
}

fn cells(line: &str) -> Vec<String> {
    line.trim().trim_matches('|').split('|').map(|c| inline(c.trim())).collect()
}
fn is_sep(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

pub fn render(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim();
        if t.is_empty() { i += 1; continue; }
        if t.starts_with("```") {
            let mut code = String::new(); i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") { code.push_str(lines[i]); code.push('\n'); i += 1; }
            i += 1;
            out.push_str(&format!("<pre><code>{}</code></pre>\n", escape(&code)));
        } else if let Some(level) = (1..=6usize).rev().find(|n| t.starts_with(&format!("{} ", "#".repeat(*n)))) {
            out.push_str(&format!("<h{0}>{1}</h{0}>\n", level + 1, inline(t[level + 1..].trim())));
            i += 1;
        } else if t.len() >= 3 && t.chars().all(|c| c == '-') || t == "***" {
            out.push_str("<hr>\n"); i += 1;
        } else if t.starts_with('>') {
            let mut q = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('>') { q.push(inline(lines[i].trim().trim_start_matches('>').trim())); i += 1; }
            out.push_str(&format!("<blockquote>{}</blockquote>\n", q.join("<br>")));
        } else if t.starts_with('|') && i + 1 < lines.len() && is_sep(lines[i + 1]) {
            out.push_str("<table><thead><tr>");
            for c in cells(t) { out.push_str(&format!("<th>{c}</th>")); }
            out.push_str("</tr></thead><tbody>"); i += 2;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                out.push_str("<tr>");
                for c in cells(lines[i]) { out.push_str(&format!("<td>{c}</td>")); }
                out.push_str("</tr>"); i += 1;
            }
            out.push_str("</tbody></table>\n");
        } else if t.starts_with("- ") || t.starts_with("* ") || t.chars().take_while(char::is_ascii_digit).count() > 0 && t.contains(". ") && t.find(". ").is_some_and(|p| t[..p].chars().all(|c| c.is_ascii_digit())) {
            let ordered = t.chars().next().is_some_and(|c| c.is_ascii_digit());
            let tag = if ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag}>"));
            while i < lines.len() {
                let u = lines[i].trim();
                let body = if !ordered && (u.starts_with("- ") || u.starts_with("* ")) { Some(&u[2..]) }
                    else if ordered { u.find(". ").filter(|p| *p > 0 && u[..*p].chars().all(|c| c.is_ascii_digit())).map(|p| &u[p + 2..]) } else { None };
                match body { Some(b) => { out.push_str(&format!("<li>{}</li>", inline(b.trim()))); i += 1; } None => break }
            }
            out.push_str(&format!("</{tag}>\n"));
        } else {
            let mut p = Vec::new();
            while i < lines.len() && !lines[i].trim().is_empty() && !lines[i].trim().starts_with("```") && !lines[i].trim().starts_with('#') { p.push(lines[i].trim()); i += 1; }
            if p.is_empty() { p.push(t); i += 1; }
            out.push_str(&format!("<p>{}</p>\n", inline(&p.join(" "))));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nothing_active_can_come_from_the_source() {
        let h = render("# T <script>alert(1)</script>\n\n[x](javascript:alert(1)) ![i](http://e/x.png) <img src=x onerror=1>\n\n`<b>`");
        assert!(!h.contains("<script") && !h.contains("<img") && !h.contains("<a ") && !h.contains("<b>"), "{h}");
        assert!(h.contains("&lt;script&gt;") && h.contains("(javascript:alert(1)") && h.contains("[image: i]"));
    }
    #[test]
    fn structure_renders() {
        let h = render("## Why\n- one\n- **two**\n\n1. a\n2. b\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```\ncode <x>\n```\n> q");
        for s in ["<h3>Why</h3>", "<ul><li>one</li><li><strong>two</strong></li></ul>", "<ol><li>a</li><li>b</li></ol>", "<th>A</th>", "<td>2</td>", "<pre><code>code &lt;x&gt;\n</code></pre>", "<blockquote>q</blockquote>"] {
            assert!(h.contains(s), "{s} in {h}");
        }
    }
    #[test]
    fn hash_is_stable() { assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"); }
}
