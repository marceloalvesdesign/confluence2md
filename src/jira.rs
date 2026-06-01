//! Jira macro/link normalization for Confluence-rendered HTML.
//!
//! The Confluence REST response may contain either storage-format Jira macros
//! or rendered `jira-issue` spans. This module rewrites both forms to simple
//! issue links without summary/status placeholder text.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::utils::{decode_html_attribute, escape_html, extract_macro_param, strip_tags};

fn jira_link_html(key: &str, href: &str) -> String {
    format!(
        r#"<a href="{}">{}</a>"#,
        escape_html(href),
        escape_html(key)
    )
}

fn jira_browse_base_url_from_href(href: &str, key: &str) -> Option<String> {
    let needle = format!("/browse/{key}");
    href.find(&needle)
        .map(|pos| href[..pos + "/browse".len()].to_owned())
}

fn find_jira_browse_base_url(html: &str) -> Option<String> {
    static RENDERED_JIRA_LINK_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r#"(?is)<span\b[^>]*\bclass="[^"]*\bjira-issue\b[^"]*"[^>]*\bdata-jira-key="([^"]+)"[^>]*>.*?<a\b[^>]*\bhref="([^"]*)""#,
        )
        .unwrap()
    });

    RENDERED_JIRA_LINK_RE.captures(html).and_then(|caps| {
        let key = decode_html_attribute(caps.get(1)?.as_str());
        let href = decode_html_attribute(caps.get(2)?.as_str());
        jira_browse_base_url_from_href(&href, &key)
    })
}

fn replace_storage_jira_macros(html: &str, jira_browse_base_url: Option<&str>) -> String {
    static STORAGE_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?is)<ac:structured-macro\b[^>]*\bac:name="jira".*?</ac:structured-macro>"#)
            .unwrap()
    });

    STORAGE_RE
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let key = extract_macro_param(&caps[0], "key").unwrap_or_default();
            match (key.is_empty(), jira_browse_base_url) {
                (true, _) => String::new(),
                (false, Some(base_url)) => jira_link_html(&key, &format!("{base_url}/{key}")),
                (false, None) => escape_html(&key),
            }
        })
        .into_owned()
}

fn rendered_jira_issue_link_html(jira_issue_span: &str) -> Option<String> {
    static LINK_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?is)<a\b[^>]*\bhref="([^"]*)"[^>]*>(.*?)</a>"#).unwrap());
    static DATA_KEY_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)data-jira-key="([^"]+)""#).unwrap());

    let link = LINK_RE.captures(jira_issue_span)?;
    let href = decode_html_attribute(link.get(1)?.as_str());
    let key = DATA_KEY_RE
        .captures(jira_issue_span)
        .and_then(|caps| caps.get(1))
        .map(|key| decode_html_attribute(key.as_str()))
        .filter(|key| !key.trim().is_empty())
        .unwrap_or_else(|| strip_tags(link.get(2).map(|m| m.as_str()).unwrap_or_default()));

    if href.is_empty() || key.is_empty() {
        None
    } else {
        Some(jira_link_html(&key, &href))
    }
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut pos = start;
    let mut quote: Option<u8> = None;
    while pos < bytes.len() {
        match (bytes[pos], quote) {
            (b'\'' | b'"', None) => quote = Some(bytes[pos]),
            (ch, Some(q)) if ch == q => quote = None,
            (b'>', None) => return Some(pos),
            _ => {}
        }
        pos += 1;
    }
    None
}

fn replace_rendered_jira_issue_spans(html: &str) -> String {
    static JIRA_SPAN_OPEN_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?is)<span\b[^>]*\bclass="[^"]*\bjira-issue\b[^"]*"[^>]*>"#).unwrap()
    });

    const SPAN_OPEN: &str = "<span";
    const SPAN_CLOSE: &str = "</span>";

    let mut out = String::with_capacity(html.len());
    let mut cursor = 0usize;

    while let Some(m) = JIRA_SPAN_OPEN_RE.find(&html[cursor..]) {
        let start = cursor + m.start();
        let mut scan_pos = cursor + m.end();
        let mut depth = 1usize;
        let mut end = None;

        while depth > 0 && scan_pos < html.len() {
            let next_open = html[scan_pos..]
                .find(SPAN_OPEN)
                .map(|offset| scan_pos + offset);
            let next_close = html[scan_pos..]
                .find(SPAN_CLOSE)
                .map(|offset| scan_pos + offset);
            match (next_open, next_close) {
                (Some(open), Some(close)) if open < close => {
                    if let Some(tag_end) = find_tag_end(html, open) {
                        let tag_text = &html[open..=tag_end];
                        if !tag_text.trim_end().ends_with("/>") {
                            depth += 1;
                        }
                        scan_pos = tag_end + 1;
                    } else {
                        break;
                    }
                }
                (_, Some(close)) => {
                    depth -= 1;
                    scan_pos = close + SPAN_CLOSE.len();
                    if depth == 0 {
                        end = Some(scan_pos);
                    }
                }
                _ => break,
            }
        }

        let Some(end) = end else {
            out.push_str(&html[cursor..cursor + m.end()]);
            cursor += m.end();
            continue;
        };

        out.push_str(&html[cursor..start]);
        let jira_issue_span = &html[start..end];
        if let Some(link) = rendered_jira_issue_link_html(jira_issue_span) {
            out.push_str(&link);
        } else {
            out.push_str(jira_issue_span);
        }
        cursor = end;
    }

    out.push_str(&html[cursor..]);
    out
}

/// Rewrite Confluence Jira macros/spans to simple issue links.
///
/// Storage-format Jira macros do not contain their browse URL, so this derives
/// the browse base URL from a rendered Jira issue link in the same REST response
/// when available. If no rendered link is available, the macro falls back to the
/// plain issue key rather than inventing an instance URL.
pub fn replace_jira_macros(html: &str) -> String {
    let jira_browse_base_url = find_jira_browse_base_url(html);
    let html = replace_storage_jira_macros(html, jira_browse_base_url.as_deref());
    replace_rendered_jira_issue_spans(&html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_jira_macro_uses_browse_url_from_rendered_issue_link() {
        let html = r#"<p><span class="jira-issue" data-jira-key="DEMO-1234"><a href="https://jira.example.com/browse/DEMO-1234" class="jira-issue-key">DEMO-1234</a></span></p><p><ac:structured-macro ac:name="jira" ac:schema-version="1" ac:macro-id="m1"><ac:parameter ac:name="server">Jira</ac:parameter><ac:parameter ac:name="key">DEMO-1235</ac:parameter></ac:structured-macro></p>"#;
        let processed = replace_jira_macros(html);
        assert!(processed.contains(
            r#"<p><a href="https://jira.example.com/browse/DEMO-1234">DEMO-1234</a></p>"#
        ));
        assert!(processed.contains(
            r#"<p><a href="https://jira.example.com/browse/DEMO-1235">DEMO-1235</a></p>"#
        ));
    }

    #[test]
    fn storage_jira_macro_without_response_link_yields_key_text() {
        let html = r#"<p><ac:structured-macro ac:name="jira" ac:schema-version="1" ac:macro-id="m1"><ac:parameter ac:name="server">Jira</ac:parameter><ac:parameter ac:name="key">DEMO-1234</ac:parameter></ac:structured-macro></p>"#;
        let processed = replace_jira_macros(html);
        assert_eq!(processed, r#"<p>DEMO-1234</p>"#);
    }

    #[test]
    fn rendered_jira_issue_yields_issue_link_only() {
        let html = r#"<p><span class="jira-issue" data-jira-key="DEMO-1234"><a href="https://jira.example.com/browse/DEMO-1234" class="jira-issue-key"><span class="aui-icon aui-icon-wait issue-placeholder"></span>DEMO-1234</a> - <span class="summary">Getting issue details...</span><span class="aui-lozenge aui-lozenge-subtle aui-lozenge-default issue-placeholder">STATUS</span></span></p>"#;
        let processed = replace_jira_macros(html);
        assert_eq!(
            processed,
            r#"<p><a href="https://jira.example.com/browse/DEMO-1234">DEMO-1234</a></p>"#
        );
    }
}
