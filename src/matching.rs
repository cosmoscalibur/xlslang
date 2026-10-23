//! Text pattern matching under the document's search settings.
//!
//! Matching is always case-insensitive: LibreOffice's "case sensitive" setting
//! governs the comparison operators only, not search criteria or lookups
//! (observed: `COUNTIF(r,"APPLE")` counts `apple` with the setting on).

use crate::options::{Search, SearchMode};
use fancy_regex::Regex;

pub enum Matcher {
    /// No special characters: compare directly.
    Plain {
        text: String,
        whole: bool,
    },
    Re(Regex),
    /// The pattern did not compile (invalid regular expression): matches nothing.
    Never,
}

fn wildcard_to_regex(p: &str) -> String {
    let mut out = String::new();
    let mut it = p.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '~' => match it.next() {
                Some(n) => out.push_str(&fancy_regex::escape(&n.to_string())),
                None => out.push('~'),
            },
            c => out.push_str(&fancy_regex::escape(&c.to_string())),
        }
    }
    out
}

impl Matcher {
    /// `whole`: the pattern must cover the entire text (criteria functions take
    /// it from the document; `SEARCH` passes `false`).
    pub fn new(pattern: &str, s: &Search, whole: bool) -> Self {
        let special = match s.mode {
            SearchMode::Wildcards => pattern.contains(['*', '?', '~']),
            SearchMode::Regex => pattern.contains(|c: char| "\\.+*?()|[]{}^$".contains(c)),
            SearchMode::Literal => false,
        };
        if !special {
            return Self::Plain {
                text: pattern.to_string(),
                whole,
            };
        }
        let body = match s.mode {
            SearchMode::Regex => pattern.to_string(),
            _ => wildcard_to_regex(pattern),
        };
        let anchored = if whole { format!("^(?:{body})$") } else { body };
        Regex::new(&format!("(?is){anchored}")).map_or(Self::Never, Self::Re)
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// Character index of the first match.
    pub fn find(&self, text: &str) -> Option<usize> {
        match self {
            Self::Never => None,
            // An empty match (`a*` against `xyz`) is not a hit, as in LibreOffice.
            Self::Re(re) => re
                .find_iter(text)
                .filter_map(Result::ok)
                .find(|m| !m.as_str().is_empty())
                .map(|m| text[..m.start()].chars().count()),
            Self::Plain { text: p, whole } => {
                let fold = |s: &str| -> String { s.chars().flat_map(char::to_lowercase).collect() };
                let (p, t) = (fold(p), fold(text));
                if *whole {
                    (p == t).then_some(0)
                } else {
                    t.find(&p).map(|b| t[..b].chars().count())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(mode: SearchMode, whole: bool, cs: bool) -> Search {
        Search {
            mode,
            whole_cell: whole,
            case_sensitive: cs,
        }
    }

    #[test]
    fn wildcards() {
        let m = Matcher::new("a*n?", &s(SearchMode::Wildcards, true, false), true);
        assert!(m.is_match("anx"));
        assert!(Matcher::new("a~*", &s(SearchMode::Wildcards, true, false), true).is_match("a*"));
        assert!(!Matcher::new("a~*", &s(SearchMode::Wildcards, true, false), true).is_match("ab"));
        assert!(!Matcher::new("a.c", &s(SearchMode::Wildcards, true, false), true).is_match("abc"));
    }

    #[test]
    fn regex_look_around_and_backreferences() {
        let re = s(SearchMode::Regex, true, false);
        assert!(Matcher::new("a(?=b)b", &re, true).is_match("ab"));
        assert!(Matcher::new("(?<!x)ab", &re, false).is_match("zab"));
        assert!(!Matcher::new("(?<!x)ab", &re, false).is_match("xab"));
        assert!(Matcher::new("(a)\\1", &re, true).is_match("aa"));
        assert!(!Matcher::new("(a)\\1", &re, true).is_match("ab"));
    }

    #[test]
    fn regex_and_literal() {
        let re = s(SearchMode::Regex, true, false);
        assert!(Matcher::new("a.c", &re, true).is_match("ABC"));
        assert!(!Matcher::new("a.", &re, true).is_match("abc"));
        assert!(Matcher::new("a.", &re, false).is_match("xabc"));
        assert!(!Matcher::new("(", &re, true).is_match("("));
        let lit = s(SearchMode::Literal, true, true);
        assert!(Matcher::new("a*", &lit, true).is_match("a*"));
        assert!(!Matcher::new("a*", &lit, true).is_match("ab"));
        // Search is case-insensitive whatever the comparison setting says.
        assert!(Matcher::new("abc", &lit, true).is_match("ABC"));
        assert!(
            !Matcher::new("a*", &re, false).is_match("xyz"),
            "empty matches do not count"
        );
    }

    #[test]
    fn whole_cell_and_position() {
        let w = s(SearchMode::Wildcards, false, false);
        assert!(Matcher::new("app", &w, false).is_match("apple"));
        assert_eq!(Matcher::new("n?n", &w, false).find("banana"), Some(2));
        assert!(!Matcher::new("app", &w, true).is_match("apple"));
    }
}
