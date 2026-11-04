// Copyright © 2026 Jalapeno Labs

//! The browser a report was made from, as one short phrase such as `Firefox 145 on Linux`.
//!
//! Only the browser's family, its major version, and the operating system's family are kept.
//! A whole `User-Agent` adds minor versions, device models, and build ids that help tell one
//! person's machine from another's and almost never help fix a bug.

/// Browser families in the order they are recognized. Order matters: Edge and Opera also say
/// `Chrome/`, and Chrome also says `Safari/`, so each is matched before what it imitates.
const BROWSERS: &[(&str, &str)] = &[
    ("Edg/", "Edge"),
    ("OPR/", "Opera"),
    ("Firefox/", "Firefox"),
    ("FxiOS/", "Firefox"),
    ("CriOS/", "Chrome"),
    ("Chrome/", "Chrome"),
    ("Version/", "Safari"),
];

/// Operating system families, matched in order: Android also says `Linux`, and iOS says
/// `Mac OS X`.
const SYSTEMS: &[(&str, &str)] = &[
    ("Android", "Android"),
    ("iPhone", "iOS"),
    ("iPad", "iPadOS"),
    ("CrOS", "ChromeOS"),
    ("Windows", "Windows"),
    ("Mac OS X", "macOS"),
    ("Linux", "Linux"),
];

/// `user_agent` as `<browser> <major version> on <system>`, or `Unknown browser` when it is
/// absent or names none of [`BROWSERS`].
pub fn describe(user_agent: Option<&str>) -> String {
    let Some(user_agent) = user_agent else {
        return "Unknown browser".to_owned();
    };
    let browser = BROWSERS.iter().find_map(|(marker, name)| {
        let (_before, after) = user_agent.split_once(marker)?;
        let major: String = after.chars().take_while(char::is_ascii_digit).collect();
        Some(if major.is_empty() {
            (*name).to_owned()
        } else {
            format!("{name} {major}")
        })
    });
    let Some(browser) = browser else {
        return "Unknown browser".to_owned();
    };
    let system = SYSTEMS
        .iter()
        .find(|(marker, _name)| user_agent.contains(marker))
        .map(|(_marker, name)| *name);
    match system {
        Some(system) => format!("{browser} on {system}"),
        None => browser,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_browsers_are_named_with_their_major_version_and_system() {
        let cases = [
            (
                "Mozilla/5.0 (X11; Linux x86_64; rv:145.0) Gecko/20100101 Firefox/145.0",
                "Firefox 145 on Linux",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/141.0.0.0 Safari/537.36",
                "Chrome 141 on Windows",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/141.0.0.0 Safari/537.36 Edg/141.0.3537.71",
                "Edge 141 on Windows",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/18.6 Safari/605.1.15",
                "Safari 18 on macOS",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1",
                "Safari 18 on iOS",
            ),
            (
                "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/141.0.0.0 Mobile Safari/537.36",
                "Chrome 141 on Android",
            ),
        ];
        for (user_agent, expected) in cases {
            assert_eq!(describe(Some(user_agent)), expected, "{user_agent}");
        }
    }

    #[test]
    fn device_models_and_build_numbers_are_left_out() {
        let described = describe(Some(
            "Mozilla/5.0 (Linux; Android 15; SM-S928B Build/AP3A.240905.015) Chrome/141.0.7390.122",
        ));
        assert_eq!(described, "Chrome 141 on Android");
    }

    #[test]
    fn an_unknown_or_missing_agent_says_so() {
        assert_eq!(describe(None), "Unknown browser");
        assert_eq!(describe(Some("curl/8.10.1")), "Unknown browser");
    }
}
