//! CI guard: every `/admin/...` URL mentioned in `docs/runbooks/*.md` must
//! correspond to a route actually registered in `crates/api-gateway/src/server.rs`.
//!
//! During an incident, operators follow the runbook verbatim. A 404 at 3am
//! because a path was renamed or never implemented is the worst possible
//! failure mode. This test makes that class of drift fail CI instead of
//! fail production.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    // crates/api-gateway → workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Extract every distinct `/admin/...` URL path referenced in the runbooks.
fn runbook_admin_paths() -> BTreeSet<String> {
    let dir = repo_root().join("docs").join("runbooks");
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(&dir).expect("docs/runbooks readable") {
        let entry = entry.unwrap();
        if entry.path().extension().map(|e| e != "md").unwrap_or(true) {
            continue;
        }
        let text = fs::read_to_string(entry.path()).unwrap();
        for (start, _) in text.match_indices("/admin/") {
            let tail: String = text[start..]
                .chars()
                .take_while(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(*c, '/' | '_' | '-' | '{' | '}')
                })
                .collect();
            // Strip any trailing slash to normalise.
            let path = tail.trim_end_matches('/').to_string();
            // Skip bare "/admin/" prefix with nothing after (e.g. LIKE pattern in SQL).
            if path == "/admin" || path == "/admin/" {
                continue;
            }
            out.insert(path);
        }
    }
    out
}

/// Parse route patterns (like `/halt` or `/users/{user_id}/orders`) out of
/// `admin_api` in `crates/api-gateway/src/server.rs`. The caller gets back
/// the fully-qualified paths (with a leading `/admin`).
fn router_admin_patterns() -> BTreeSet<String> {
    let src = repo_root()
        .join("crates")
        .join("api-gateway")
        .join("src")
        .join("server.rs");
    let text = fs::read_to_string(&src).expect("server.rs readable");

    // A route line looks like:  .route("/users/{user_id}/orders", get(rest::admin_user_orders))
    // We find every occurrence of `.route("` and capture the quoted path.
    let mut out = BTreeSet::new();
    for (start, _) in text.match_indices(".route(\"") {
        let after = &text[start + ".route(\"".len()..];
        if let Some(end) = after.find('"') {
            let pat = &after[..end];
            if pat.starts_with('/') {
                out.insert(format!("/admin{}", pat));
            }
        }
    }
    out
}

/// Returns true iff `actual` matches `pattern` segment-by-segment, where
/// any `{placeholder}` segment in the pattern matches any non-empty actual
/// segment.
fn path_matches(pattern: &str, actual: &str) -> bool {
    let p: Vec<&str> = pattern.trim_start_matches('/').split('/').collect();
    let a: Vec<&str> = actual.trim_start_matches('/').split('/').collect();
    if p.len() != a.len() {
        return false;
    }
    p.iter().zip(a.iter()).all(|(pp, aa)| {
        (pp.starts_with('{') && pp.ends_with('}')) || pp == aa
    })
}

#[test]
fn every_runbook_admin_path_exists_in_router() {
    let runbook_paths = runbook_admin_paths();
    let router_patterns = router_admin_patterns();

    assert!(
        !runbook_paths.is_empty(),
        "extracted zero /admin paths from docs/runbooks/*.md — regex is broken"
    );
    assert!(
        !router_patterns.is_empty(),
        "extracted zero /admin routes from server.rs — regex is broken"
    );

    let mut missing: Vec<String> = Vec::new();
    for path in &runbook_paths {
        let found = router_patterns.iter().any(|pat| path_matches(pat, path));
        if !found {
            missing.push(path.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "runbook endpoints missing from admin router:\n  {}\n\n\
        router patterns (first 20):\n  {}\n\n\
        Fix options:\n  \
        1. Update the runbook to use a real endpoint.\n  \
        2. Implement the missing endpoint in server.rs + rest.rs.\n  \
        3. If the runbook intentionally documents a direct-DB workaround, \
        make sure the code block does not contain a `/admin/...` URL that \
        would be picked up by this test.",
        missing.join("\n  "),
        router_patterns
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn path_matches_basic_cases() {
    assert!(path_matches("/admin/halt", "/admin/halt"));
    assert!(path_matches(
        "/admin/users/{user_id}/orders",
        "/admin/users/abc-123/orders"
    ));
    assert!(path_matches("/admin/pairs/{symbol}", "/admin/pairs/BTCUSDT"));
    assert!(!path_matches("/admin/pairs/{symbol}", "/admin/pairs"));
    assert!(!path_matches(
        "/admin/engine/halt",
        "/admin/engine/halt/extra"
    ));
}
