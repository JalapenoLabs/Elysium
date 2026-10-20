// Copyright © 2026 Jalapeno Labs

//! What the updater should do now, decided from what the API knows. Pure, so every rule is
//! tested without a database, a registry, or a clock.

use semver::Version;
use serde::Serialize;

/// Everything the decision depends on.
#[derive(Debug, Clone, Copy)]
pub struct Situation<'a> {
    /// The release this API is, or `None` for a build that is not a release.
    pub running: Option<&'a Version>,
    /// The newest release the registry holds, when a check has found one.
    pub newest: Option<&'a Version>,
    /// A release an admin asked for, which is applied even if it rolled back before.
    pub requested: Option<&'a Version>,
    /// Releases that were applied and rolled back.
    pub rolled_back: &'a [Version],
    /// Whether an update is applying now.
    pub applying: bool,
    /// Whether nothing is running that a restart would cut short.
    pub idle: bool,
}

/// Where the deployment stands, as Settings shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateState {
    /// This build is not a release, such as `main` or a development build, so it never
    /// updates itself.
    NotARelease,
    /// Nothing newer to apply.
    UpToDate,
    /// The newest release rolled back here before, and waits for an admin to ask for it.
    HeldBack,
    /// A release is waiting for running work to finish.
    WaitingForWork,
    /// The updater is applying a release.
    Applying,
    /// The updater may apply a release now.
    Ready,
}

/// The state, and the release to apply when it is [`UpdateState::Ready`] or waiting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub state: UpdateState,
    pub target: Option<Version>,
}

/// Decides what the updater should do.
///
/// An admin's request wins over the automatic choice, which is the newest release when it
/// is newer than the running one and has not rolled back here. Nothing applies while an
/// update is applying or while work is running.
pub fn decide(situation: Situation<'_>) -> Decision {
    let Some(running) = situation.running else {
        return Decision {
            state: UpdateState::NotARelease,
            target: None,
        };
    };

    if situation.applying {
        return Decision {
            state: UpdateState::Applying,
            target: None,
        };
    }

    let newer = situation.newest.filter(|newest| *newest > running);
    let automatic = newer.filter(|newest| !situation.rolled_back.contains(newest));
    let Some(target) = situation.requested.or(automatic) else {
        let state = if newer.is_some() {
            UpdateState::HeldBack
        } else {
            UpdateState::UpToDate
        };
        return Decision {
            state,
            target: None,
        };
    };

    let state = if situation.idle {
        UpdateState::Ready
    } else {
        UpdateState::WaitingForWork
    };
    Decision {
        state,
        target: Some(target.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("a valid version")
    }

    fn situation<'a>(running: &'a Version, newest: &'a Version) -> Situation<'a> {
        Situation {
            running: Some(running),
            newest: Some(newest),
            requested: None,
            rolled_back: &[],
            applying: false,
            idle: true,
        }
    }

    #[test]
    fn a_newer_release_is_applied_when_nothing_is_running() {
        let running = version("0.1.0");
        let newest = version("0.1.1");
        let decision = decide(situation(&running, &newest));
        assert_eq!(decision.state, UpdateState::Ready);
        assert_eq!(decision.target, Some(newest));
    }

    #[test]
    fn running_work_holds_the_update() {
        let running = version("0.1.0");
        let newest = version("0.1.1");
        let decision = decide(Situation {
            idle: false,
            ..situation(&running, &newest)
        });
        assert_eq!(decision.state, UpdateState::WaitingForWork);
        assert_eq!(decision.target, Some(newest));
    }

    #[test]
    fn the_same_or_an_older_release_is_up_to_date() {
        let running = version("0.2.0");
        for newest in [version("0.2.0"), version("0.1.9")] {
            let decision = decide(situation(&running, &newest));
            assert_eq!(decision.state, UpdateState::UpToDate);
            assert_eq!(decision.target, None);
        }
    }

    #[test]
    fn a_build_that_is_not_a_release_never_updates() {
        let newest = version("9.0.0");
        let decision = decide(Situation {
            running: None,
            ..situation(&newest, &newest)
        });
        assert_eq!(decision.state, UpdateState::NotARelease);
        assert_eq!(decision.target, None);
    }

    #[test]
    fn nothing_new_applies_while_an_update_is_applying() {
        let running = version("0.1.0");
        let newest = version("0.1.1");
        let decision = decide(Situation {
            applying: true,
            ..situation(&running, &newest)
        });
        assert_eq!(decision.state, UpdateState::Applying);
        assert_eq!(decision.target, None);
    }

    #[test]
    fn a_release_that_rolled_back_is_held_until_an_admin_asks_for_it() {
        let running = version("0.1.0");
        let newest = version("0.1.1");
        let rolled_back = [newest.clone()];

        let held = decide(Situation {
            rolled_back: &rolled_back,
            ..situation(&running, &newest)
        });
        assert_eq!(held.state, UpdateState::HeldBack);
        assert_eq!(held.target, None);

        let requested = decide(Situation {
            rolled_back: &rolled_back,
            requested: Some(&newest),
            ..situation(&running, &newest)
        });
        assert_eq!(requested.state, UpdateState::Ready);
        assert_eq!(requested.target, Some(newest));
    }

    #[test]
    fn a_newer_release_than_the_one_that_rolled_back_applies_on_its_own() {
        let running = version("0.1.0");
        let newest = version("0.1.2");
        let rolled_back = [version("0.1.1")];
        let decision = decide(Situation {
            rolled_back: &rolled_back,
            ..situation(&running, &newest)
        });
        assert_eq!(decision.target, Some(newest));
    }

    #[test]
    fn releases_compare_by_version_not_by_text() {
        let running = version("0.9.0");
        let newest = version("0.10.0");
        let decision = decide(situation(&running, &newest));
        assert_eq!(decision.target, Some(newest));
    }
}
