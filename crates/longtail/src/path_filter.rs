//! `RegexPathFilter` — golongtail-compatible include/exclude path filtering
//! (`longtailutils/pathfilter.go`).
//!
//! Multiple regexes are packed into one string separated by `**` (a `\` escapes
//! a following `*` so it is not treated as a separator). Matching is **unanchored
//! substring** (Go's `regexp.MatchString`). Exclude wins over include; with no
//! include regexes, everything not excluded is included.

use regex::Regex;

use crate::error::LongtailError;

/// A compiled include/exclude path filter.
#[derive(Debug, Default)]
pub struct RegexPathFilter {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

/// Split a `**`-separated multi-regex string and compile each piece, mirroring
/// Go's `splitRegexes` state machine (a `\` suppresses the next `*` from being
/// counted toward a `**` separator).
fn split_regexes(s: &str) -> Result<Vec<Regex>, LongtailError> {
    let bytes = s.as_bytes();
    let mut out: Vec<Regex> = Vec::new();
    let mut m: i32 = 0;
    let mut start: usize = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' {
            m = -1;
        } else if m == 0 && c == b'*' {
            m = 1;
        } else if m == 1 && c == b'*' {
            let piece = &s[start..i - 1];
            out.push(compile(piece)?);
            start = i + 1;
            m = 0;
        } else {
            m = 0;
        }
        i += 1;
    }
    if start < bytes.len() {
        out.push(compile(&s[start..])?);
    }
    Ok(out)
}

fn compile(pattern: &str) -> Result<Regex, LongtailError> {
    Regex::new(pattern).map_err(|e| {
        LongtailError::InvalidArgument(format!("invalid path filter regex `{pattern}`: {e}"))
    })
}

impl RegexPathFilter {
    /// Build from optional include/exclude filter strings. `None`/empty on both
    /// yields an include-everything filter.
    pub fn new(
        include_filter: Option<&str>,
        exclude_filter: Option<&str>,
    ) -> Result<RegexPathFilter, LongtailError> {
        let include = match include_filter {
            Some(s) if !s.is_empty() => split_regexes(s)?,
            _ => Vec::new(),
        };
        let exclude = match exclude_filter {
            Some(s) if !s.is_empty() => split_regexes(s)?,
            _ => Vec::new(),
        };
        Ok(RegexPathFilter { include, exclude })
    }

    /// Whether this filter does any filtering at all.
    pub fn is_noop(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Whether `asset_path` (root-relative, no trailing slash) should be
    /// included. For directories the trailing-`/` form is also tested
    /// (pathfilter.go:65-91).
    pub fn include(&self, asset_path: &str, is_dir: bool) -> bool {
        let dir_path = if is_dir {
            Some(format!("{asset_path}/"))
        } else {
            None
        };
        for r in &self.exclude {
            if r.is_match(asset_path) {
                return false;
            }
            if let Some(dp) = &dir_path
                && r.is_match(dp)
            {
                return false;
            }
        }
        if self.include.is_empty() {
            return true;
        }
        for r in &self.include {
            if r.is_match(asset_path) {
                return true;
            }
            if let Some(dp) = &dir_path
                && r.is_match(dp)
            {
                return true;
            }
        }
        false
    }
}
