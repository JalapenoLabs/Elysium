// Copyright © 2026 Jalapeno Labs

//! What `bw serve` answers, and what agents are shown of it.
//!
//! Only the fields Elysium reads are declared; the CLI's responses carry many more, which serde
//! skips. Every item is checked against the collection agents may read before anything of it
//! leaves this module ([`in_collection`]), whatever filter the CLI was asked to apply: the
//! filter decides what is fetched, and this check decides what is shown.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bitwarden's item type for a login (`CipherType.Login`).
const LOGIN_TYPE: u8 = 1;

/// The most items a search answers, so a vague search cannot flood the agent's context.
pub const MAX_SEARCH_RESULTS: usize = 25;

/// Every `bw serve` answer: `success`, and either `data` or the CLI's `message`.
#[derive(Debug, Deserialize)]
pub struct ServeResponse<Data> {
    pub success: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default = "Option::default")]
    pub data: Option<Data>,
}

/// A list answer, `{ "object": "list", "data": [...] }`.
#[derive(Debug, Deserialize)]
pub struct List<Item> {
    pub data: Vec<Item>,
}

/// A text answer, such as a TOTP code: `{ "object": "string", "data": "123456" }`.
#[derive(Debug, Deserialize)]
pub struct Text {
    pub data: String,
}

/// `GET /status` and `bw status`: `{ "object": "template", "template": {...} }`.
#[derive(Debug, Deserialize)]
pub struct StatusTemplate {
    pub template: Status,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub user_email: Option<String>,
    #[serde(rename = "status")]
    pub lock_state: LockState,
}

/// Where the CLI stands with its account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockState {
    Unauthenticated,
    Locked,
    Unlocked,
}

/// A collection the account reaches, as `bw list collections` answers it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
}

/// A vault item, as `bw get item` and `bw list items` answer it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub collection_ids: Vec<Uuid>,
    #[serde(default)]
    pub deleted_date: Option<String>,
    #[serde(default)]
    pub login: Option<Login>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Login {
    #[serde(default)]
    pub username: Option<String>,
    /// `None` when the item hides its password from this account.
    #[serde(default)]
    pub password: Option<String>,
    /// The TOTP seed. Never shown to an agent: Elysium hands out the current code instead.
    #[serde(default)]
    pub totp: Option<String>,
    #[serde(default)]
    pub uris: Option<Vec<LoginUri>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginUri {
    #[serde(default)]
    pub uri: Option<String>,
}

/// Whether agents may read `item`: a login, not in the trash, in `collection`.
pub fn in_collection(item: &Item, collection: Uuid) -> bool {
    item.kind == LOGIN_TYPE
        && item.deleted_date.is_none()
        && item.collection_ids.contains(&collection)
}

/// An item as a search shows it: everything but its secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: Uuid,
    pub name: String,
    pub username: Option<String>,
    pub uris: Vec<String>,
    pub has_totp: bool,
}

/// An item as `passwords_login` shows it: its username and password, for signing in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDetails {
    pub id: Uuid,
    pub name: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub uris: Vec<String>,
    pub has_totp: bool,
}

fn uris(login: &Login) -> Vec<String> {
    login
        .uris
        .iter()
        .flatten()
        .filter_map(|uri| uri.uri.clone())
        .collect()
}

impl ItemSummary {
    pub fn new(item: &Item) -> Self {
        let login = item.login.clone().unwrap_or_default();
        Self {
            id: item.id,
            name: item.name.clone(),
            username: login.username.clone(),
            uris: uris(&login),
            has_totp: login.totp.is_some(),
        }
    }
}

impl LoginDetails {
    pub fn new(item: &Item) -> Self {
        let login = item.login.clone().unwrap_or_default();
        Self {
            id: item.id,
            name: item.name.clone(),
            username: login.username.clone(),
            password: login.password.clone(),
            uris: uris(&login),
            has_totp: login.totp.is_some(),
        }
    }
}

/// The items of `items` agents may read, as summaries, at most [`MAX_SEARCH_RESULTS`].
pub fn summaries(items: &[Item], collection: Uuid) -> Vec<ItemSummary> {
    items
        .iter()
        .filter(|item| in_collection(item, collection))
        .take(MAX_SEARCH_RESULTS)
        .map(ItemSummary::new)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ELYSIA: Uuid = Uuid::from_u128(0xe1);
    const OTHER: Uuid = Uuid::from_u128(0x07);

    /// An item as the CLI prints it, trimmed to the fields that matter here plus a few it adds.
    fn item(collections: &[Uuid], kind: u8, deleted: bool) -> Item {
        serde_json::from_value(json!({
            "object": "item",
            "id": "8c7f5c0e-0d1e-4b5e-9e2b-1f6a0b2c3d4e",
            "organizationId": "1f6a0b2c-3d4e-4b5e-9e2b-8c7f5c0e0d1e",
            "folderId": null,
            "type": kind,
            "reprompt": 0,
            "name": "Pizza Palace",
            "notes": "the notes are never shown",
            "favorite": false,
            "login": {
                "uris": [{ "match": null, "uri": "https://pizza.example.com/login" }],
                "username": "alex@example.com",
                "password": "hunter2-but-longer",
                "totp": "JBSWY3DPEHPK3PXP",
                "passwordRevisionDate": null
            },
            "collectionIds": collections,
            "revisionDate": "2026-10-01T12:00:00.000Z",
            "creationDate": "2026-09-01T12:00:00.000Z",
            "deletedDate": if deleted { json!("2026-10-02T12:00:00.000Z") } else { json!(null) },
            "passwordHistory": null
        }))
        .expect("the CLI's item shape parses")
    }

    #[test]
    fn only_live_logins_in_the_collection_are_readable() {
        assert!(in_collection(&item(&[ELYSIA], 1, false), ELYSIA));
        assert!(in_collection(&item(&[OTHER, ELYSIA], 1, false), ELYSIA));
        assert!(!in_collection(&item(&[OTHER], 1, false), ELYSIA));
        assert!(
            !in_collection(&item(&[], 1, false), ELYSIA),
            "a personal item"
        );
        assert!(
            !in_collection(&item(&[ELYSIA], 1, true), ELYSIA),
            "in the trash"
        );
        assert!(
            !in_collection(&item(&[ELYSIA], 2, false), ELYSIA),
            "a secure note"
        );
    }

    #[test]
    fn a_summary_carries_no_secret() {
        let summary = ItemSummary::new(&item(&[ELYSIA], 1, false));
        let shown = serde_json::to_string(&summary).expect("serializes");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(!shown.contains("JBSWY3DP"), "{shown}");
        assert!(!shown.contains("notes"), "{shown}");
        assert_eq!(summary.uris, ["https://pizza.example.com/login"]);
        assert!(summary.has_totp);
    }

    #[test]
    fn login_details_carry_the_password_but_never_the_totp_seed() {
        let details = LoginDetails::new(&item(&[ELYSIA], 1, false));
        assert_eq!(details.password.as_deref(), Some("hunter2-but-longer"));
        let shown = serde_json::to_string(&details).expect("serializes");
        assert!(!shown.contains("JBSWY3DP"), "{shown}");
    }

    #[test]
    fn search_results_are_filtered_and_bounded() {
        let mut items = vec![item(&[OTHER], 1, false)];
        items.extend((0..40).map(|_| item(&[ELYSIA], 1, false)));
        let found = summaries(&items, ELYSIA);
        assert_eq!(found.len(), MAX_SEARCH_RESULTS);
    }

    #[test]
    fn serve_answers_parse_with_and_without_data() {
        let refused: ServeResponse<Item> = serde_json::from_value(json!({
            "success": false,
            "message": "Vault is locked."
        }))
        .expect("parses");
        assert!(!refused.success);
        assert_eq!(refused.message.as_deref(), Some("Vault is locked."));

        let status: ServeResponse<StatusTemplate> = serde_json::from_value(json!({
            "success": true,
            "data": {
                "object": "template",
                "template": {
                    "serverUrl": "https://vault.example.com",
                    "lastSync": "2026-10-21T08:00:00.000Z",
                    "userEmail": "elysia@example.com",
                    "userId": "1f6a0b2c-3d4e-4b5e-9e2b-8c7f5c0e0d1e",
                    "status": "unlocked"
                }
            }
        }))
        .expect("parses");
        let template = status.data.expect("data").template;
        assert_eq!(template.lock_state, LockState::Unlocked);
        assert_eq!(template.user_email.as_deref(), Some("elysia@example.com"));
    }
}
