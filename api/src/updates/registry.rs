// Copyright © 2026 Jalapeno Labs

//! Which releases Docker Hub holds for the API image.
//!
//! The updater pulls from Docker Hub, so the registry is the source of truth: a release
//! counts once its image is there, not when its git tag is pushed, which happens about
//! twenty minutes earlier. Docker Hub's host is fixed in code.

use semver::Version;
use serde::Deserialize;
use url::Url;

/// Docker Hub's API.
const DOCKER_HUB_ORIGIN: &str = "https://hub.docker.com";

/// The API image's tags that contain a dot: releases (`1.2.3`), their floating minors
/// (`1.2`), and pre-releases. `main` and `sha-<commit>` contain none, so the listing stays
/// short however often `main` is published.
const RELEASE_TAGS_PATH: &str =
    "/v2/namespaces/jalapenolabs/repositories/elysium-api/tags?page_size=100&name=.";

/// A bound on pages followed, so a registry that kept answering `next` cannot hold a check
/// forever. A hundred tags a page covers years of daily releases.
const MAX_PAGES: usize = 20;

#[derive(Debug, Deserialize)]
struct TagPage {
    next: Option<String>,
    results: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
}

/// Why the registry could not be read.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Docker Hub could not be reached: {0}")]
    Unreachable(#[source] reqwest::Error),
    #[error("Docker Hub answered {0}")]
    Refused(reqwest::StatusCode),
    #[error("Docker Hub answered with a listing Elysium cannot read: {0}")]
    Unreadable(String),
}

/// Reads the API image's releases from Docker Hub.
#[derive(Debug, Clone)]
pub struct Registry {
    http: reqwest::Client,
    origin: Url,
}

impl Registry {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            origin: Url::parse(DOCKER_HUB_ORIGIN).expect("the Docker Hub origin is a valid URL"),
        }
    }

    #[cfg(test)]
    pub fn with_origin(http: reqwest::Client, origin: Url) -> Self {
        Self { http, origin }
    }

    /// The newest stable release Docker Hub holds, or `None` before the first release.
    ///
    /// # Errors
    /// Fails when Docker Hub cannot be reached, refuses, or answers with something that is
    /// not a tag listing, including a `next` page on another host.
    pub async fn newest_release(&self) -> Result<Option<Version>, RegistryError> {
        let mut page_url = self
            .origin
            .join(RELEASE_TAGS_PATH)
            .map_err(|error| RegistryError::Unreadable(error.to_string()))?;
        let mut newest: Option<Version> = None;

        for _ in 0..MAX_PAGES {
            let response = self
                .http
                .get(page_url.clone())
                .send()
                .await
                .map_err(|error| RegistryError::Unreachable(error.without_url()))?;
            if !response.status().is_success() {
                return Err(RegistryError::Refused(response.status()));
            }
            let page: TagPage = response
                .json()
                .await
                .map_err(|error| RegistryError::Unreadable(error.without_url().to_string()))?;

            let names = page.results.iter().map(|tag| tag.name.as_str());
            newest = newest.max(newest_stable(names));

            let Some(next) = page.next else {
                return Ok(newest);
            };
            page_url = self.same_origin(&next)?;
        }

        Err(RegistryError::Unreadable(format!(
            "the listing ran past {MAX_PAGES} pages"
        )))
    }

    /// Follows a `next` link only on Docker Hub itself.
    fn same_origin(&self, link: &str) -> Result<Url, RegistryError> {
        let url = Url::parse(link).map_err(|error| RegistryError::Unreadable(error.to_string()))?;
        if url.origin() != self.origin.origin() {
            return Err(RegistryError::Unreadable(format!(
                "the next page is on another host: {}",
                url.host_str().unwrap_or_default()
            )));
        }
        Ok(url)
    }
}

/// The newest stable release among tag names. Floating tags (`1.2`, `latest`) are not
/// versions, and pre-releases (`1.2.3-rc.1`) are never applied automatically.
pub fn newest_stable<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Version> {
    names
        .into_iter()
        .filter_map(|name| Version::parse(name).ok())
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
        .max()
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::extract::{Query, State};
    use axum::routing::get;
    use serde_json::{Value, json};
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn the_newest_stable_release_wins_over_floating_tags_and_pre_releases() {
        let names = [
            "0.1",
            "latest",
            "0.1.9",
            "0.1.10",
            "0.2.0-rc.1",
            "0.1.10+build",
        ];
        assert_eq!(newest_stable(names), Some(Version::new(0, 1, 10)));
    }

    #[test]
    fn no_release_is_none() {
        assert_eq!(newest_stable(["main", "sha-3fb90adcd6c4"]), None);
    }

    /// Serves two pages of tags, the second only when asked for page 2.
    async fn serve_pages(next_host: &'static str) -> Url {
        async fn tags(
            State(base): State<String>,
            Query(query): Query<HashMap<String, String>>,
        ) -> axum::Json<Value> {
            if query.get("page").map(String::as_str) == Some("2") {
                return axum::Json(json!({ "next": null, "results": [{ "name": "0.3.0" }] }));
            }
            axum::Json(json!({
                "next": format!("{base}/v2/namespaces/jalapenolabs/repositories/elysium-api/tags?page=2"),
                "results": [{ "name": "0.2.0" }, { "name": "0.2" }],
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port");
        let address = listener.local_addr().expect("a bound address");
        let base = format!("http://{next_host}:{}", address.port());
        let app = Router::new()
            .route(
                "/v2/namespaces/jalapenolabs/repositories/elysium-api/tags",
                get(tags),
            )
            .with_state(base);
        tokio::spawn(async move { axum::serve(listener, app).await });
        Url::parse(&format!("http://127.0.0.1:{}", address.port())).expect("a valid URL")
    }

    #[tokio::test]
    async fn every_page_is_read() {
        let origin = serve_pages("127.0.0.1").await;
        let registry = Registry::with_origin(reqwest::Client::new(), origin);
        let newest = registry
            .newest_release()
            .await
            .expect("the listing is read");
        assert_eq!(newest, Some(Version::new(0, 3, 0)));
    }

    #[tokio::test]
    async fn a_next_page_on_another_host_is_refused() {
        let origin = serve_pages("localhost").await;
        let registry = Registry::with_origin(reqwest::Client::new(), origin);
        let error = registry
            .newest_release()
            .await
            .expect_err("another host is not followed");
        assert!(matches!(error, RegistryError::Unreadable(_)), "{error}");
    }
}
