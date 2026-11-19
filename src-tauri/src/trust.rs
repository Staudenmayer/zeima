//! What the app believes of the addresses a server tells it about. A server (or someone in the middle of an
//! unencrypted development connection) hands the app the address of the sign-in service, the API and the pages to open
//! in the browser. They must be what the person typed or part of the same site, and must not be anything the
//! operating system would treat as more than a web page.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::account::{Discovery, OidcConfig};

/// `localhost`, `127.x.x.x` and `::1`: this machine, where a development server needs no certificate.
pub fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
        || host.parse::<std::net::Ipv6Addr>().is_ok_and(|ip| ip.is_loopback())
}

/// The registrable domain (`id.zeima.example.co.uk` → `example.co.uk`), by the full Public Suffix List (hosting
/// platforms such as `github.io` or `blogspot.com` are suffixes too: two hosts below one are two strangers). An address
/// given as numbers, or a name the list does not know, is its own.
fn registrable(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return host;
    }
    match psl::domain_str(&host) {
        Some(domain) => domain.to_string(),
        None => host,
    }
}

/// Whether two hosts belong to the same site: equal, one inside the other, or under one registrable domain.
pub fn same_site(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    a == b || registrable(&a) == registrable(&b)
}

fn parse(label: &str, raw: &str) -> Result<Url, String> {
    Url::parse(raw).map_err(|_| format!("the {label} address the server gave is not valid"))
}

/// https, or plain http for this machine (only where `local` says the server itself is on this machine).
fn check_scheme(label: &str, url: &Url, local: bool) -> Result<(), String> {
    let host = url.host_str().unwrap_or_default();
    match url.scheme() {
        "https" => Ok(()),
        "http" if local && is_loopback(host) => Ok(()),
        _ => Err(format!("the {label} address the server gave does not use https")),
    }
}

/// What the server's discovery document may say: https addresses on the server's own site.
pub fn check_discovery(server_url: &str, discovery: &Discovery) -> Result<(), String> {
    let server = Url::parse(server_url).map_err(|_| "that is not a valid address".to_string())?;
    let server_host = server.host_str().unwrap_or_default().to_string();
    let local = is_loopback(&server_host);
    for (label, raw) in [("sign-in", discovery.issuer.as_str()), ("API", discovery.api_url.as_str())] {
        let url = parse(label, raw)?;
        check_scheme(label, &url, local)?;
        let host = url.host_str().unwrap_or_default();
        // A development server on this machine may split its services over ports and names; a real one stays on its site.
        if !local && !same_site(host, &server_host) {
            return Err(format!(
                "the {label} service of this server is on another website ({host}) than the server ({server_host}); the app does not trust it"
            ));
        }
    }
    Ok(())
}

/// What the sign-in service's own document may say: the endpoints are https and on the issuer's site.
pub fn check_oidc(server_url: &str, issuer: &str, oidc: &OidcConfig) -> Result<(), String> {
    let local = Url::parse(server_url).ok().and_then(|u| u.host_str().map(is_loopback)).unwrap_or(false);
    let issuer_url = parse("sign-in", issuer)?;
    let issuer_host = issuer_url.host_str().unwrap_or_default().to_string();
    for (label, raw) in [("authorization", oidc.authorization_endpoint.as_str()), ("token", oidc.token_endpoint.as_str())] {
        let url = parse(label, raw)?;
        check_scheme(label, &url, local)?;
        let host = url.host_str().unwrap_or_default();
        if !local && !same_site(host, &issuer_host) {
            return Err(format!("the sign-in service names a {label} endpoint on another website ({host}); the app does not trust it"));
        }
    }
    Ok(())
}

/// A link the app may hand to the system: a web page (https, or http on this machine) or a mail address. Anything else
/// (`file:`, the schemes other programs register) is refused, whoever supplied it.
pub fn checked_link(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| "that link is not valid".to_string())?;
    match url.scheme() {
        "https" | "mailto" => Ok(url),
        "http" if url.host_str().is_some_and(is_loopback) => Ok(url),
        other => Err(format!("links of the kind \"{other}:\" are not opened")),
    }
}

/// Opens a link in the browser after `checked_link`.
pub fn open_link(app: &AppHandle, raw: &str) -> Result<(), String> {
    let url = checked_link(raw)?;
    app.opener().open_url(url.as_str(), None::<&str>).map_err(|e| format!("could not open the browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovery(issuer: &str, api: &str) -> Discovery {
        Discovery { api_url: api.into(), issuer: issuer.into(), client_id: "c".into(), api_versions: vec![1], min_client_version: None }
    }
    fn oidc(auth: &str, token: &str) -> OidcConfig {
        OidcConfig { authorization_endpoint: auth.into(), token_endpoint: token.into() }
    }

    #[test]
    fn loopback_is_this_machine_only() {
        for h in ["localhost", "127.0.0.1", "127.9.9.9", "[::1]", "::1", "app.localhost"] {
            assert!(is_loopback(h), "{h}");
        }
        for h in ["example.com", "10.0.0.1", "localhost.example.com", "192.168.1.1", "0.0.0.0"] {
            assert!(!is_loopback(h), "{h}");
        }
    }

    #[test]
    fn registrable_domains_count_two_labels_or_three_under_a_public_suffix() {
        assert_eq!(registrable("id.zeima.example.com"), "example.com");
        assert_eq!(registrable("example.com"), "example.com");
        assert_eq!(registrable("id.firma.co.uk"), "firma.co.uk");
        assert_eq!(registrable("a.b.example.com.au"), "example.com.au");
        assert_eq!(registrable("10.1.2.3"), "10.1.2.3");
        assert!(same_site("id.example.com", "time.example.com"));
        assert!(same_site("example.com", "id.example.com"));
        assert!(!same_site("example.com", "evil.com"));
        assert!(!same_site("a.co.uk", "b.co.uk"));
        assert!(!same_site("a.vercel.app", "b.vercel.app"));
        assert!(!same_site("x.github.io", "y.github.io"));
        assert!(same_site("id.x.github.io", "time.x.github.io"));
        for (a, b) in [("a.blogspot.com", "b.blogspot.com"), ("a.ts.net", "b.ts.net"), ("a.web.app", "b.web.app"), ("a.netlify.app", "b.netlify.app")] {
            assert!(!same_site(a, b), "{a} {b}");
        }
        assert!(!same_site("example.com", "example.com.evil.com"));
    }

    #[test]
    fn a_server_may_name_its_own_sign_in_and_api_over_https() {
        let ok = discovery("https://id.zeima.example.com/realms/zeima", "https://zeima.example.com");
        assert!(check_discovery("https://zeima.example.com", &ok).is_ok());
        // Sibling hosts of one site are fine.
        assert!(check_discovery("https://time.example.com", &discovery("https://login.example.com/realms/z", "https://time.example.com")).is_ok());
    }

    #[test]
    fn a_server_may_not_send_the_app_elsewhere() {
        let server = "https://zeima.example.com";
        let elsewhere = discovery("https://login.evil.com/realms/zeima", "https://zeima.example.com");
        assert!(check_discovery(server, &elsewhere).unwrap_err().contains("another website"));
        let api_elsewhere = discovery("https://id.example.com/realms/zeima", "https://evil.com");
        assert!(check_discovery(server, &api_elsewhere).is_err());
        let plain = discovery("http://id.example.com/realms/zeima", "https://zeima.example.com");
        assert!(check_discovery(server, &plain).unwrap_err().contains("https"));
        let lookalike = discovery("https://example.com.evil.com/realms/zeima", "https://zeima.example.com");
        assert!(check_discovery(server, &lookalike).is_err());
        assert!(check_discovery(server, &discovery("file:///etc/passwd", "https://zeima.example.com")).is_err());
    }

    #[test]
    fn a_development_server_on_this_machine_may_use_http_and_other_ports() {
        let dev = discovery("http://localhost:8080/realms/zeima", "http://localhost:3000");
        assert!(check_discovery("http://localhost:3000", &dev).is_ok());
        // ... but a remote server cannot claim a loopback http address, or any http address.
        assert!(check_discovery("https://zeima.example.com", &dev).is_err());
        let sneaky = discovery("http://id.example.com/realms/zeima", "http://zeima.example.com");
        assert!(check_discovery("http://localhost:3000", &sneaky).is_err());
    }

    #[test]
    fn the_sign_in_endpoints_stay_on_the_issuers_site_over_https() {
        let server = "https://zeima.example.com";
        let issuer = "https://id.example.com/realms/zeima";
        assert!(check_oidc(server, issuer, &oidc("https://id.example.com/realms/zeima/auth", "https://id.example.com/realms/zeima/token")).is_ok());
        assert!(check_oidc(server, issuer, &oidc("https://phish.example.net/auth", "https://id.example.com/token")).is_err());
        assert!(check_oidc(server, issuer, &oidc("https://id.example.com/auth", "http://id.example.com/token")).is_err());
        assert!(check_oidc("http://localhost:3000", "http://localhost:8080/realms/zeima", &oidc("http://localhost:8080/a", "http://localhost:8080/t")).is_ok());
    }

    #[test]
    fn only_web_pages_and_mail_addresses_are_opened() {
        assert!(checked_link("https://store.example.com/checkout/1").is_ok());
        assert!(checked_link("mailto:help@example.com").is_ok());
        assert!(checked_link("http://localhost:5173/x").is_ok());
        for bad in ["file:///etc/passwd", "http://example.com/", "javascript:alert(1)", "ms-msdt:/id", "steam://run/1", "smb://host/share", "not a url", ""] {
            assert!(checked_link(bad).is_err(), "{bad}");
        }
    }
}
