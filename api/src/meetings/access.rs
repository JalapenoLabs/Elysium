// Copyright © 2026 Jalapeno Labs

//! What a person may do with a meeting: read it, change it, manage it, or what only its owner
//! does. Every meeting route asks [`open`] for the meeting it names with what it needs, so the
//! rule lives here and in `models::meeting::readable_by` alone, never route by route. See
//! `docs/meeting-review.md`, Who sees a meeting.
//!
//! | Need             | Who                                      | For                               |
//! |------------------|------------------------------------------|-----------------------------------|
//! | [`Need::Read`]   | Whoever `readable_by` lets read it       | Its words, captures, media, history, and threads |
//! | [`Need::Edit`]   | A reader who is a company member, and its owner | Edits, threads, excerpts and black-outs, and undoing them |
//! | [`Need::Manage`] | Its owner, or an admin                   | Who sees it, and merging or splitting its captures |
//! | [`Need::Own`]    | Its owner                                | Its processing: holding, approving, retrying, Done reviewing, offsets, and captures it uploads |
//!
//! A meeting someone may not read answers as one that does not exist, so a private meeting's
//! existence is no answer either; one they read but may not change in that way names what it
//! takes.

use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use crate::models::meeting::{self, Meeting, Reach, Reader};
use crate::models::meeting_event::EventKind;
use crate::models::user::User;

/// What a request needs of the meeting it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Read,
    Edit,
    Manage,
    Own,
}

/// What `reader` may do with one meeting they may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access {
    /// Changes its words and its threads: any company member who reads it, and its owner. A guest
    /// reads a meeting shared with them and changes nothing.
    pub edits: bool,
    /// Changes who sees it, and merges or splits its captures: its owner, and admins.
    pub manages: bool,
    /// Its owner, who alone runs its processing.
    pub owns: bool,
}

impl Access {
    /// What `reader`, who may read `meeting`, may do with it.
    pub fn of(meeting: &Meeting, reader: Reader) -> Self {
        let owns = meeting.created_by == reader.id;
        Self {
            edits: owns || matches!(reader.reach, Reach::Every | Reach::Public),
            manages: owns || reader.reach == Reach::Every,
            owns,
        }
    }

    /// Whether it meets `need`.
    pub const fn meets(self, need: Need) -> bool {
        match need {
            Need::Read => true,
            Need::Edit => self.edits,
            Need::Manage => self.manages,
            Need::Own => self.owns,
        }
    }
}

/// Why a meeting was refused to someone.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// It does not exist, or they may not read it.
    #[error("not found")]
    NotFound,
    /// They read it, and it takes more than they may: what it takes.
    #[error("the meeting needs {0:?}")]
    Needs(Need),
    #[error(transparent)]
    Database(diesel::result::Error),
}

/// The meeting `id` with what `user` may do with it, when they may read it and it meets `need`.
///
/// # Errors
/// [`Refusal::NotFound`] when it does not exist or they may not read it, [`Refusal::Needs`] when
/// they read it but it takes more than they may, and any database error.
pub async fn open(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    user: &User,
    need: Need,
) -> Result<(Meeting, Access), Refusal> {
    let reader = Reader::of(user);
    let found = match meeting::find_readable(connection, id, reader).await {
        Ok(found) => found,
        Err(diesel::result::Error::NotFound) => return Err(Refusal::NotFound),
        Err(error) => return Err(Refusal::Database(error)),
    };
    let access = Access::of(&found, reader);
    if !access.meets(need) {
        return Err(Refusal::Needs(need));
    }
    Ok((found, access))
}

/// What undoing a change of kind `kind` takes: what making it took. A kind that cannot be undone
/// takes only an editor's, and its undo refuses it as one.
pub const fn need_to_undo(kind: Option<EventKind>) -> Need {
    match kind {
        Some(
            EventKind::Merged
            | EventKind::Split
            | EventKind::VisibilityChanged
            | EventKind::Shared
            | EventKind::Unshared,
        ) => Need::Manage,
        Some(EventKind::HoldChanged | EventKind::OffsetChanged) => Need::Own,
        Some(
            EventKind::Edited
            | EventKind::Marked
            | EventKind::Unmarked
            | EventKind::ThreadResolved
            | EventKind::ThreadReopened
            | EventKind::Built
            | EventKind::Settled
            | EventKind::CorrectionApproved
            | EventKind::CaptureAdded
            | EventKind::Segmented
            | EventKind::ReviewDone
            | EventKind::Attached
            | EventKind::Detached,
        )
        | None => Need::Edit,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::models::meeting::Visibility;

    const OWNER: Uuid = Uuid::from_u128(1);
    const SOMEONE: Uuid = Uuid::from_u128(2);

    fn meeting() -> Meeting {
        Meeting {
            id: Uuid::from_u128(9),
            title: "One to one".to_owned(),
            started_at: Utc::now(),
            duration_ms: None,
            organizer: String::new(),
            participants: json!([]),
            segments: None,
            state: "done".to_owned(),
            created_by: OWNER,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            hold_for_approval: false,
            merged_into: None,
            visibility: Visibility::Private.as_str().to_owned(),
            visibility_item_id: None,
        }
    }

    fn reader(id: Uuid, reach: Reach) -> Reader {
        Reader { id, reach }
    }

    #[test]
    fn a_member_who_reads_a_meeting_edits_it_and_only_its_owner_and_admins_manage_it() {
        let found = meeting();
        let owner = Access::of(&found, reader(OWNER, Reach::Public));
        assert_eq!(
            owner,
            Access {
                edits: true,
                manages: true,
                owns: true
            }
        );
        let member = Access::of(&found, reader(SOMEONE, Reach::Public));
        assert!(member.meets(Need::Read) && member.meets(Need::Edit));
        assert!(!member.meets(Need::Manage) && !member.meets(Need::Own));
        let admin = Access::of(&found, reader(SOMEONE, Reach::Every));
        assert!(admin.meets(Need::Edit) && admin.meets(Need::Manage));
        assert!(
            !admin.meets(Need::Own),
            "an admin runs no one else's processing"
        );
    }

    #[test]
    fn a_guest_reads_a_meeting_shared_with_them_and_changes_nothing() {
        let guest = Access::of(&meeting(), reader(SOMEONE, Reach::Shared));
        assert!(guest.meets(Need::Read));
        for need in [Need::Edit, Need::Manage, Need::Own] {
            assert!(!guest.meets(need), "{need:?}");
        }
    }

    #[test]
    fn a_guest_who_owns_a_meeting_does_everything_its_owner_does() {
        let owner = Access::of(&meeting(), reader(OWNER, Reach::Shared));
        for need in [Need::Read, Need::Edit, Need::Manage, Need::Own] {
            assert!(owner.meets(need), "{need:?}");
        }
    }

    #[test]
    fn undoing_a_change_takes_what_making_it_took() {
        assert_eq!(need_to_undo(Some(EventKind::Edited)), Need::Edit);
        assert_eq!(need_to_undo(Some(EventKind::ThreadResolved)), Need::Edit);
        assert_eq!(need_to_undo(Some(EventKind::Merged)), Need::Manage);
        assert_eq!(
            need_to_undo(Some(EventKind::VisibilityChanged)),
            Need::Manage
        );
        assert_eq!(need_to_undo(Some(EventKind::Unshared)), Need::Manage);
        assert_eq!(need_to_undo(Some(EventKind::HoldChanged)), Need::Own);
        assert_eq!(need_to_undo(Some(EventKind::OffsetChanged)), Need::Own);
    }
}
