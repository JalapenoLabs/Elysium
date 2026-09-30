// Copyright © 2026 Jalapeno Labs

//! Which image a Studio item's tile shows.
//!
//! A pinned image always wins. Otherwise the newest image whose name ends in `-hero`, the
//! three-quarter view Studio's instructions ask agents to render after every visible change,
//! and failing that the newest image of any name. An item with no image yet has none, and
//! the grid shows a placeholder while its first turn runs.

use uuid::Uuid;

use crate::models::studio_asset::{StudioAsset, StudioAssetKind};

/// The suffix, before the extension, that marks a presentation render.
const HERO_SUFFIX: &str = "-hero";

/// The thumbnail of an item pinned to `pinned`, whose files are `assets`, newest first.
pub fn thumbnail_of(pinned: Option<Uuid>, assets: &[&StudioAsset]) -> Option<Uuid> {
    if pinned.is_some() {
        return pinned;
    }

    let mut newest_image = None;
    for asset in assets {
        if asset.kind != StudioAssetKind::Image {
            continue;
        }
        if is_hero(&asset.artifact_path) {
            return Some(asset.id);
        }
        newest_image = newest_image.or(Some(asset.id));
    }
    newest_image
}

/// Whether a path names a hero render: its file name, without the extension, ends in `-hero`.
fn is_hero(artifact_path: &str) -> bool {
    let name = artifact_path.rsplit('/').next().unwrap_or(artifact_path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _extension)| stem);
    stem.to_ascii_lowercase().ends_with(HERO_SUFFIX)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn asset(path: &str, kind: StudioAssetKind) -> StudioAsset {
        StudioAsset {
            id: Uuid::now_v7(),
            studio_item_id: Uuid::nil(),
            session_id: None,
            kind,
            artifact_path: path.to_owned(),
            content_type: None,
            size_bytes: 1,
            sha256: "0".repeat(64),
            storage_path: "studio/x".to_owned(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_pinned_image_wins() {
        let hero = asset("renders/banana-hero.png", StudioAssetKind::Image);
        let pinned = Uuid::now_v7();
        assert_eq!(thumbnail_of(Some(pinned), &[&hero]), Some(pinned));
    }

    #[test]
    fn the_newest_hero_render_beats_newer_images_of_other_names() {
        // Newest first, as the item's assets are listed.
        let top = asset("renders/banana-top.png", StudioAssetKind::Image);
        let hero = asset("renders/banana-HERO.png", StudioAssetKind::Image);
        let older_hero = asset("renders/banana-hero.png", StudioAssetKind::Image);
        assert_eq!(thumbnail_of(None, &[&top, &hero, &older_hero]), Some(hero.id));
    }

    #[test]
    fn without_a_hero_the_newest_image_is_shown_and_models_never() {
        let model = asset("banana.glb", StudioAssetKind::Model);
        let front = asset("renders/banana-front.png", StudioAssetKind::Image);
        let side = asset("renders/banana-side.png", StudioAssetKind::Image);
        assert_eq!(thumbnail_of(None, &[&model, &front, &side]), Some(front.id));
        assert_eq!(thumbnail_of(None, &[&model]), None);
        assert_eq!(thumbnail_of(None, &[]), None);
    }

    #[test]
    fn hero_is_read_from_the_file_name_alone() {
        assert!(is_hero("renders/banana-hero.png"));
        assert!(is_hero("banana-hero"));
        assert!(!is_hero("hero/banana.png"), "a directory named hero is not a hero render");
        assert!(!is_hero("superhero-poster.png"));
    }
}
