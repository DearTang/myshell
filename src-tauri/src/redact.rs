//! Sensitive-field redaction for log output.
//!
//! Passwords / private keys are never logged in the first place (they only
//! live transiently in `ConnectionConfig` and the keyring). What CAN leak
//! through logs are connection *hosts*, *usernames*, and *proxy addresses* —
//! metadata that isn't a credential but is still private to the user (you
//! may not want your server IPs / hostnames shipped to us in a feedback
//! report). This module masks those before they reach `log::`.
//!
//! Strategy ("standard" redaction):
//!   - IP literal  → keep first + last segment, mask the middle:
//!       `192.168.1.10` → `192.*.*.10`
//!   - hostname    → keep first + last char, mask the middle:
//!       `myserver.com` → `m*********m`
//!   - username    → keep first + last char, mask the middle:
//!       `admin` → `a***n`
//!   - ≤2 chars    → `***` (not enough surface to partially reveal)
//!
//! Ports are NOT masked — they aren't personally identifying and are useful
//! for debugging (e.g. distinguishing :22 vs :2222).

/// True if the string looks like a dotted-quad IPv4 literal.
fn is_ipv4_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Mask the middle of a string, keeping the first and last character.
/// Strings of length ≤ 2 collapse to `***`.
fn mask_middle(s: &str) -> String {
    let len = s.chars().count();
    if len <= 2 {
        return "***".to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let first = chars[0];
    let last = chars[len - 1];
    let masked = "*".repeat(len - 2);
    format!("{}{}{}", first, masked, last)
}

/// Redact a host (hostname or IP literal).
///
/// IPv4 keeps the first + last octet so a log reader can still tell two
/// different machines apart (`10.*.*.5` vs `10.*.*.9`) without learning the
/// full address. Hostnames get first + last char.
pub fn host(h: &str) -> String {
    let h = h.trim();
    if h.is_empty() {
        return String::new();
    }
    if is_ipv4_like(h) {
        let parts: Vec<&str> = h.split('.').collect();
        return format!("{}.*.*.{}", parts[0], parts[3]);
    }
    mask_middle(h)
}

/// Redact a username: keep first + last char, mask the middle.
pub fn user(u: &str) -> String {
    let u = u.trim();
    if u.is_empty() {
        return String::new();
    }
    mask_middle(u)
}

/// Best-effort secondary scrub for log text that was written by code paths
/// we don't control (russh internals, or a log file from a build before this
/// redaction existed). Runs simple regexes to catch:
///   - bare IPv4 literals  → `a.b.c.d` masked
///   - `user@host` SSH-style targets → mask both halves
///
/// This is deliberately conservative: it only rewrites things that match
/// narrow patterns, so it won't corrupt arbitrary log prose. It's a
/// defence-in-depth pass on top of the point-of-origin masking above.
pub fn scrub_log_text(text: &str) -> String {
    use std::sync::OnceLock;
    use regex::Regex;

    static IPV4: OnceLock<Regex> = OnceLock::new();
    static SSH_TARGET: OnceLock<Regex> = OnceLock::new();

    let ipv4 = IPV4.get_or_init(|| {
        Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap()
    });
    let ssh_target =
        SSH_TARGET.get_or_init(|| Regex::new(r"\b([A-Za-z0-9._-]+)@([A-Za-z0-9._-]+)\b").unwrap());

    let mut out = text.to_string();

    // IPv4: keep first + last octet.
    out = ipv4
        .replace_all(&out, |c: &regex::Captures| {
            format!("{}.*.*.{}", &c[1], &c[4])
        })
        .to_string();

    // user@host targets — but only the SSH-style ones (skip email by requiring
    // the host part to not look like a mail domain heuristic isn't reliable,
    // so we just mask both halves; an email address leaking into an SSH log is
    // rare and masking it is the safe call).
    out = ssh_target
        .replace_all(&out, |c: &regex::Captures| {
            format!("{}@{}", user(&c[1]), host(&c[2]))
        })
        .to_string();

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_keeps_first_and_last() {
        assert_eq!(host("192.168.1.10"), "192.*.*.10");
        assert_eq!(host("10.0.0.5"), "10.*.*.5");
    }

    #[test]
    fn hostname_keeps_ends() {
        // myserver.com = 12 chars → 10 masked middle chars
        assert_eq!(host("myserver.com"), "m**********m");
        // prod-db-01 = 10 chars → 8 masked middle chars
        assert_eq!(host("prod-db-01"), "p********1");
    }

    #[test]
    fn short_strings_collapse() {
        assert_eq!(host("a"), "***");
        assert_eq!(user("ab"), "***");
        assert_eq!(host(""), "");
    }

    #[test]
    fn user_masks_middle() {
        assert_eq!(user("admin"), "a***n");
        assert_eq!(user("root"), "r**t");
    }

    #[test]
    fn scrub_handles_ipv4_and_target() {
        let s = "connecting to 192.168.1.10 as admin@prod-server";
        let scrubbed = scrub_log_text(s);
        assert!(scrubbed.contains("192.*.*.10"));
        // admin(5)→a***n, prod-server(11)→p*********r
        assert!(scrubbed.contains("a***n@p*********r"));
        assert!(!scrubbed.contains("192.168.1.10"));
    }
}
