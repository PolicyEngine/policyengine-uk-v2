//! Tree-shaped read path for parameter YAMLs.
//!
//! Today the canonical parameter loader (`Parameters::for_year`) deserializes
//! a year's YAML into the hand-coded `Parameters` struct. Adding a new field
//! requires a Rust edit, recompile, and new struct field — friction for any
//! contributor who knows the parameter tree but doesn't write Rust.
//!
//! `ParameterTree` is a parallel read path that exposes the *same* year YAML
//! as a generic `serde_yaml::Value` indexable by dot-separated path, so a
//! parameter can be added to YAML and read out by code without first being
//! mirrored as a struct field.
//!
//! Existing behaviour is unchanged — the engine still reads from `Parameters`.
//! Future slices of #50 may rewrite `Parameters::for_year` on top of this
//! loader; for now the two coexist.
//!
//! # Example
//!
//! ```ignore
//! use crate::parameters::ParameterTree;
//!
//! let tree = ParameterTree::for_year(2025).unwrap();
//! let pa = tree.lookup_f64("income_tax.personal_allowance").unwrap();
//! assert_eq!(pa, 12_570.0);
//!
//! // Nested paths just keep traversing.
//! let main_rate = tree.lookup_f64("national_insurance.main_rate").unwrap();
//! ```

use std::path::{Path, PathBuf};

use serde_yaml::Value;

/// A flat-YAML parameter year exposed as a generic value tree.
///
/// Wraps the parsed root `Value`. Lookups walk the YAML tree by dot-separated
/// path; sequence indices use `[N]` (e.g. `income_tax.uk_brackets[1].rate`).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ParameterTree {
    root: Value,
}

#[allow(dead_code)]
impl ParameterTree {
    /// Load the YAML for `year` (start of fiscal year — e.g. 2025 for 2025/26).
    /// Resolves against `parameters/<year>_<year+1>.yaml` first, falling back
    /// to the manifest-rooted path used in development.
    pub fn for_year(year: u32) -> anyhow::Result<Self> {
        let filename = format!("{}_{:02}.yaml", year, (year + 1) % 100);
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("parameters").join(&filename),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parameters").join(&filename),
        ];
        for path in &candidates {
            if path.exists() {
                return Self::from_path(path);
            }
        }
        anyhow::bail!(
            "No parameter file for fiscal year {}/{}; looked for: {}",
            year, year + 1,
            candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "),
        )
    }

    /// Load from a specific path.
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_yaml(&contents)
    }

    /// Build from a YAML string.
    pub fn from_yaml(yaml_str: &str) -> anyhow::Result<Self> {
        let root: Value = serde_yaml::from_str(yaml_str)?;
        Ok(ParameterTree { root })
    }

    /// Look up a value by dot-separated path. Returns `None` for missing keys
    /// or when the path traverses into a non-mapping/non-sequence node.
    ///
    /// Sequence indices are written `[N]`, e.g.
    /// `income_tax.uk_brackets[1].rate`.
    pub fn lookup(&self, path: &str) -> Option<&Value> {
        let mut cur = &self.root;
        for segment in split_path(path) {
            cur = match (segment, cur) {
                (PathSegment::Key(k), Value::Mapping(m)) => m.get(Value::String(k.into())).as_ref().copied()?,
                (PathSegment::Index(i), Value::Sequence(s)) => s.get(i)?,
                _ => return None,
            };
        }
        Some(cur)
    }

    /// Typed lookup — returns `None` if the path is missing or the value is
    /// not numeric. Convenience wrapper around `lookup(...).and_then(as_f64)`.
    pub fn lookup_f64(&self, path: &str) -> Option<f64> {
        self.lookup(path).and_then(value_as_f64)
    }

    /// Typed lookup — returns `None` if the path is missing or the value is
    /// not a string.
    #[allow(dead_code)]
    pub fn lookup_str(&self, path: &str) -> Option<&str> {
        self.lookup(path).and_then(Value::as_str)
    }

    /// Borrow the underlying YAML value for callers that want to walk the
    /// tree directly (e.g. iterating over the brackets sequence).
    #[allow(dead_code)]
    pub fn root(&self) -> &Value {
        &self.root
    }
}

/// `serde_yaml::Value` exposes integers and floats separately; we want either
/// to count.
#[allow(dead_code)]
fn value_as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)).or_else(|| v.as_u64().map(|i| i as f64))
}

#[derive(Debug, PartialEq)]
enum PathSegment<'a> {
    Key(&'a str),
    Index(usize),
}

/// Split `"a.b[1].c"` into `[Key("a"), Key("b"), Index(1), Key("c")]`.
/// Empty segments and malformed indices are dropped — the lookup will then
/// miss, which is the same outcome as a typo'd key.
fn split_path(path: &str) -> Vec<PathSegment<'_>> {
    let mut out = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        // Split a single dot-separated segment that may carry one or more
        // `[N]` index suffixes, e.g. `uk_brackets[1]` or `matrix[0][2]`.
        let (head, mut rest) = match part.find('[') {
            Some(i) => (&part[..i], &part[i..]),
            None => (part, ""),
        };
        if !head.is_empty() {
            out.push(PathSegment::Key(head));
        }
        while let Some(close) = rest.find(']') {
            let inner = &rest[1..close]; // skip leading `[`
            if let Ok(i) = inner.parse::<usize>() {
                out.push(PathSegment::Index(i));
            } else {
                return Vec::new(); // malformed → empty path → miss
            }
            rest = &rest[close + 1..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> ParameterTree {
        ParameterTree::from_yaml(
            r#"
income_tax:
  personal_allowance: 12570.0
  uk_brackets:
    - { rate: 0.20, threshold: 0.0 }
    - { rate: 0.40, threshold: 37700.0 }
    - { rate: 0.45, threshold: 125140.0 }
fiscal_year: "2025/26"
"#,
        )
        .unwrap()
    }

    #[test]
    fn lookup_top_level_string() {
        let t = sample_tree();
        assert_eq!(t.lookup_str("fiscal_year"), Some("2025/26"));
    }

    #[test]
    fn lookup_nested_scalar() {
        let t = sample_tree();
        assert_eq!(t.lookup_f64("income_tax.personal_allowance"), Some(12_570.0));
    }

    #[test]
    fn lookup_sequence_index() {
        let t = sample_tree();
        assert_eq!(t.lookup_f64("income_tax.uk_brackets[1].threshold"), Some(37_700.0));
        assert_eq!(t.lookup_f64("income_tax.uk_brackets[2].rate"), Some(0.45));
    }

    #[test]
    fn lookup_missing_key_returns_none() {
        let t = sample_tree();
        assert!(t.lookup("income_tax.no_such_field").is_none());
        assert!(t.lookup_f64("income_tax.no_such_field").is_none());
    }

    #[test]
    fn lookup_out_of_range_index_returns_none() {
        let t = sample_tree();
        assert!(t.lookup_f64("income_tax.uk_brackets[99].rate").is_none());
    }

    #[test]
    fn malformed_index_returns_none() {
        let t = sample_tree();
        assert!(t.lookup_f64("income_tax.uk_brackets[xyz].rate").is_none());
    }

    #[test]
    fn loads_real_2025_26_yaml() {
        // Sanity: the loader resolves the actual year file shipped with the
        // crate, and a known parameter reads back correctly.
        let t = ParameterTree::for_year(2025).unwrap();
        assert_eq!(t.lookup_f64("income_tax.personal_allowance"), Some(12_570.0));
        // National Insurance main rate post-2024 reduction.
        assert_eq!(t.lookup_f64("national_insurance.main_rate"), Some(0.08));
    }
}
