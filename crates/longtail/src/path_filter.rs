//! `RegexPathFilter` — golongtail-compatible include/exclude path filtering
//! (`longtailutils/pathfilter.go`).
//!
//! Multiple regexes are packed into one string separated by `**` (a `\` escapes
//! a following `*` so it is not treated as a separator). Matching is **unanchored
//! substring** (Go's `regexp.MatchString`). Exclude wins over include; with no
//! include regexes, everything not excluded is included.

use std::path::{Component, Path};

use regex::Regex;

use crate::error::LongtailError;

/// Root-relative, `/`-separated form of `path` when it lies inside `root`.
///
/// Best-effort: canonicalised first so `.`, `..` and symlinked roots resolve, and
/// falling back to a textual strip when either side does not exist yet (an index
/// file is routinely named before it is written). A path outside `root` returns
/// `None` — it cannot be scanned as part of that folder, so nothing needs saying
/// about it.
pub(crate) fn relative_within(root: &Path, path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let rel = match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(r), Ok(c)) => c.strip_prefix(&r).ok().map(Path::to_path_buf),
        _ => candidate.strip_prefix(root).ok().map(Path::to_path_buf),
    }?;
    if rel.as_os_str().is_empty() || rel.components().any(|c| c == Component::ParentDir) {
        return None;
    }
    let joined = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    (!joined.is_empty()).then_some(joined)
}

/// The name of the target-index cache `downsync` writes inside the target folder
/// when `cache_target_index` is on.
///
/// Declared here rather than beside the code that writes it, because both users
/// need the same string for opposite reasons: `downsync` builds the path to read
/// and write it, and the filter has to keep it out of every version index.
pub const TARGET_INDEX_CACHE_NAME: &str = ".longtail.index.cache.lvi";

/// A compiled include/exclude path filter.
#[derive(Debug, Default)]
pub struct RegexPathFilter {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
    /// Root-relative paths that are this tool's own index files rather than
    /// content, and so are never scanned into a version index. Checked ahead of
    /// the regexes: an include filter cannot opt one back in.
    never: Vec<String>,
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
        Ok(RegexPathFilter {
            include,
            exclude,
            never: Vec::new(),
        })
    }

    /// Treat `paths` (root-relative, `/`-separated) as this tool's own index
    /// files rather than content: never scanned, whatever the regexes say.
    ///
    /// A version index is a description of a folder, so a folder cannot
    /// meaningfully contain its own. The target-index cache does live inside the
    /// target, and indexing it there propagates one machine's cache to every
    /// consumer of the version — where it is re-materialised as content and, on a
    /// run that fails partway, left behind claiming to describe the target.
    #[must_use]
    pub fn never_paths(mut self, paths: impl IntoIterator<Item = String>) -> RegexPathFilter {
        self.never.extend(paths);
        self
    }

    /// Whether this filter does any filtering at all.
    pub fn is_noop(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty() && self.never.is_empty()
    }

    /// Whether `asset_path` is one of the never-content paths.
    ///
    /// Compared case-insensitively so a case-preserving-but-insensitive
    /// filesystem cannot smuggle the file past as `.LongTail.Index.Cache.lvi`.
    pub fn is_never_content(&self, asset_path: &str) -> bool {
        self.never
            .iter()
            .any(|p| p.eq_ignore_ascii_case(asset_path))
    }

    /// Whether `asset_path` (root-relative, no trailing slash) should be
    /// included. For directories the trailing-`/` form is also tested
    /// (pathfilter.go:65-91).
    pub fn include(&self, asset_path: &str, is_dir: bool) -> bool {
        if self.is_never_content(asset_path) {
            return false;
        }
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

#[cfg(test)]
mod never_content_tests {
    use std::path::Path;

    use super::{RegexPathFilter, TARGET_INDEX_CACHE_NAME, relative_within};

    /// A never-content path is not an exclude regex: an include filter that would
    /// otherwise select it must not bring it back. Someone filtering for `*.lvi`
    /// is asking for content, not for the tool's own bookkeeping.
    #[test]
    fn an_include_filter_cannot_opt_the_cache_back_in() {
        let f = RegexPathFilter::new(Some(r"\.lvi"), None)
            .unwrap()
            .never_paths([TARGET_INDEX_CACHE_NAME.to_string()]);
        assert!(!f.include(TARGET_INDEX_CACHE_NAME, false));
        assert!(
            f.include("assets/level.lvi", false),
            "real content still passes"
        );
    }

    /// Case-insensitive, so a case-preserving-but-insensitive filesystem cannot
    /// present the same file under a spelling that slips past.
    #[test]
    fn the_match_ignores_case() {
        let f = RegexPathFilter::new(None, None)
            .unwrap()
            .never_paths([TARGET_INDEX_CACHE_NAME.to_string()]);
        assert!(!f.include(".LongTail.Index.Cache.LVI", false));
        // A same-named file in a subfolder is content: the cache is root-relative.
        assert!(f.include("sub/.longtail.index.cache.lvi", false));
    }

    #[test]
    fn relative_within_only_accepts_paths_under_the_root() {
        let root = Path::new("/data/target");
        assert_eq!(
            relative_within(root, "/data/target/idx/current.lvi").as_deref(),
            Some("idx/current.lvi")
        );
        // Outside the root, so it cannot be scanned as part of it.
        assert_eq!(relative_within(root, "/data/other/current.lvi"), None);
        assert_eq!(relative_within(root, "/data/target"), None);
        // Escaping components are refused rather than normalised away.
        assert_eq!(relative_within(root, "/data/target/../x.lvi"), None);
    }
}
