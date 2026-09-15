// Copyright © 2026 Jalapeno Labs

//! The only two pages the broker renders: the consent interstitial and a problem page.
//!
//! The interstitial is the broker's defense against consent phishing. Anyone can
//! start a flow naming any return address, so before a provider is involved the
//! person signing in is shown, in large type, which Elysium instance will receive
//! access to their mailbox. Both pages are served with a policy that forbids framing,
//! so the Continue button cannot be clickjacked from another site.

use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use html_escape::{encode_double_quoted_attribute, encode_text};

/// Shared page shell. No scripts, no external resources.
const STYLE: &str = "\
body{margin:0;font:16px/1.5 system-ui,sans-serif;background:#0f1419;color:#e6e6e6;\
display:flex;min-height:100vh;align-items:center;justify-content:center;padding:1rem}\
main{max-width:32rem;background:#1a2027;border:1px solid #2d3640;border-radius:12px;padding:2rem}\
h1{font-size:1.25rem;margin:0 0 1rem}p{margin:0 0 1rem;color:#b8c0c8}\
.host{display:block;font:600 1.25rem ui-monospace,monospace;color:#fff;background:#0f1419;\
border-radius:8px;padding:.75rem 1rem;margin:0 0 1rem;overflow-wrap:anywhere}\
.actions{display:flex;gap:.75rem;flex-wrap:wrap}\
a.button{padding:.6rem 1.1rem;border-radius:8px;text-decoration:none;font-weight:600}\
a.primary{background:#3b82f6;color:#fff}a.secondary{color:#b8c0c8;border:1px solid #2d3640}";

/// Asks the person to confirm which instance they are connecting their mailbox to.
pub fn consent(
    instance_host: &str,
    provider_name: &str,
    continue_url: &str,
    cancel_url: &str,
) -> Response {
    let body = format!(
        "<h1>Connect your {provider} mailbox to Elysium</h1>\
         <p>This Elysium instance is asking to read and send email as you:</p>\
         <span class=\"host\">{host}</span>\
         <p>Only continue if you started this from that address. It will be able to read, send, \
         and delete mail in the account you choose.</p>\
         <div class=\"actions\">\
         <a class=\"button primary\" href=\"{continue_url}\">Continue to {provider}</a>\
         <a class=\"button secondary\" href=\"{cancel_url}\">Cancel</a>\
         </div>",
        provider = encode_text(provider_name),
        host = encode_text(instance_host),
        continue_url = encode_double_quoted_attribute(continue_url),
        cancel_url = encode_double_quoted_attribute(cancel_url),
    );
    page(StatusCode::OK, "Connect your mailbox", &body)
}

/// Explains why a flow cannot continue, for failures with nowhere safe to redirect.
pub fn problem(title: &str, detail: &str) -> Response {
    let body = format!(
        "<h1>{title}</h1><p>{detail}</p>",
        title = encode_text(title),
        detail = encode_text(detail),
    );
    page(StatusCode::BAD_REQUEST, title, &body)
}

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let document = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title><style>{STYLE}</style></head><body><main>{body}</main></body></html>",
        title = encode_text(title),
    );

    (
        status,
        [
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; form-action 'none'",
            ),
            (header::X_FRAME_OPTIONS, "DENY"),
            // The page's links carry sealed state; no referrer should repeat them.
            (header::REFERRER_POLICY, "no-referrer"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Html(document),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf-8")
    }

    #[tokio::test]
    async fn the_consent_page_escapes_everything_it_is_given() {
        let response = consent(
            "evil.example<script>",
            "Google",
            "https://accounts.google.com/?a=1&b=\"2\"",
            "https://evil.example/cb?error=access_denied",
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");

        let html = body_of(response).await;
        assert!(html.contains("evil.example&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("a=1&amp;b=&quot;2&quot;"));
    }

    #[test]
    fn a_problem_page_is_a_client_error() {
        assert_eq!(
            problem("Nope", "Because.").status(),
            StatusCode::BAD_REQUEST
        );
    }
}
