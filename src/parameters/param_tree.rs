//! Tree-shaped parameter loader mirroring the Python `policyengine-uk`
//! parameter system (addresses issue #50).
//!
//! Python stores parameters as a *directory tree* of YAML files under
//! `policyengine_uk/parameters/gov/...`. Each leaf YAML is one of three node
//! types:
//!
//!   * `values:` — a date-keyed map of scalar values (the value in force on a
//!     given day is the latest entry on or before that day).
//!   * `brackets:` — a list of `{ rate, threshold }` brackets, where each of
//!     `rate` and `threshold` is itself a date-keyed value map.
//!   * `scales:` — a synonym some parameter sets use for `brackets:`; handled
//!     identically.
//!
//! Directories become interior nodes, so a leaf is addressed by the
//! dot-joined path of directory names + file stem, e.g.
//! `gov.hmrc.income_tax.rates.basic_rate`. This matches Python's
//! `parameters.gov.hmrc.income_tax.rates.basic_rate(period)` access pattern.
//!
//! This loader lands **additively**: it does not replace the hand-coded
//! [`crate::parameters::Parameters`] struct or the flat year-files that every
//! `compute_*` consumer reads today. Migrating those consumers onto path-keyed
//! tree lookups is a follow-up (see issue #50).
//!
//! # Example
//!
//! ```ignore
//! use crate::parameters::ParamTree;
//!
//! let tree = ParamTree::from_dir("tests/fixtures/parameters")?;
//! // Value in force on the requested date (latest on-or-before).
//! let basic_rate = tree.get_at("gov.hmrc.income_tax.rates.basic_rate", 2025).unwrap();
//! assert_eq!(basic_rate, 0.20);
//! ```

// The tree loader lands additively (issue #50): its API is exercised by the
// test suite and is intended for the follow-up consumer migration, so the bin
// target does not yet call every item. Suppress dead-code noise for the module.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use serde_yaml::Value;

/// A calendar date, stored as a sortable `(year, month, day)` triple so that
/// ordering and "latest on or before" comparisons are trivial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// First instant of a fiscal/calendar year, used when a caller requests a
    /// parameter "in year Y" without a finer date.
    pub fn start_of_year(year: i32) -> Self {
        Date { year, month: 1, day: 1 }
    }

    /// Parse an ISO `YYYY-MM-DD` date key as used in the parameter YAMLs.
    fn parse(s: &str) -> Option<Date> {
        let mut parts = s.split('-');
        let year = parts.next()?.parse::<i32>().ok()?;
        let month = parts.next()?.parse::<u8>().ok()?;
        let day = parts.next()?.parse::<u8>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Date { year, month, day })
    }
}

/// A date-keyed map of scalar values. The value "at" a date is the value of the
/// latest key on or before that date (Python's step-function semantics).
///
/// `null` entries (used in Python to clear a value for a window) are stored as
/// `None`, so a lookup returns `None` for a date inside such a window.
#[derive(Debug, Clone, Default)]
pub struct ValueMap {
    /// Sorted by date ascending; `BTreeMap` keeps the ordering for us.
    entries: BTreeMap<Date, Option<f64>>,
}

impl ValueMap {
    fn from_yaml(value: &Value) -> Option<ValueMap> {
        let map = value.as_mapping()?;
        let mut entries = BTreeMap::new();
        for (k, v) in map {
            let date = Date::parse(k.as_str()?)?;
            let scalar = value_as_f64(v); // None for `null`
            entries.insert(date, scalar);
        }
        Some(ValueMap { entries })
    }

    /// The value in force on `date`: the latest entry on or before `date`.
    /// Returns `None` if no entry precedes the date or the entry is `null`.
    pub fn get_at(&self, date: Date) -> Option<f64> {
        self.entries
            .range(..=date)
            .next_back()
            .and_then(|(_, v)| *v)
    }

    /// The most recent (date, value) entry that has a non-null value, used as
    /// the base for forward uprating.
    fn last_known(&self) -> Option<(Date, f64)> {
        self.entries
            .iter()
            .rev()
            .find_map(|(d, v)| v.map(|val| (*d, val)))
    }

    /// The latest declared date key (regardless of value), used to decide
    /// whether a requested date falls beyond the table and so needs uprating.
    fn last_date(&self) -> Option<Date> {
        self.entries.keys().next_back().copied()
    }

    /// The earliest declared date key.
    fn first_date(&self) -> Option<Date> {
        self.entries.keys().next().copied()
    }
}

/// One bracket of a `brackets:` / `scales:` node: a date-keyed rate and a
/// date-keyed threshold.
#[derive(Debug, Clone)]
pub struct Bracket {
    pub rate: ValueMap,
    pub threshold: ValueMap,
}

/// A parameter tree node.
#[derive(Debug, Clone)]
pub enum Node {
    /// A `values:` leaf, optionally auto-uprated by another path's index.
    Values {
        values: ValueMap,
        /// Dot-path of the index used to uprate forward (Python `metadata.uprating`),
        /// or `None`. The literal `self` is mapped to `None` here because a
        /// self-uprated series carries no separate index series.
        uprating: Option<String>,
    },
    /// A `brackets:` / `scales:` leaf.
    Brackets(Vec<Bracket>),
    /// An interior directory node.
    Subtree(BTreeMap<String, Node>),
}

/// The loaded parameter tree, rooted at the parameters directory.
#[derive(Debug, Clone)]
pub struct ParamTree {
    root: Node,
}

impl ParamTree {
    /// Walk a directory tree of parameter YAMLs and parse it into a typed tree.
    ///
    /// Directories become [`Node::Subtree`]s; `.yaml` files become leaf nodes
    /// keyed by their file stem. Non-YAML files are ignored.
    pub fn from_dir(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = load_dir(dir.as_ref())?;
        Ok(ParamTree { root })
    }

    /// Borrow the node at a dot-separated path, or `None` if it is missing.
    pub fn node(&self, path: &str) -> Option<&Node> {
        let mut cur = &self.root;
        for segment in path.split('.').filter(|s| !s.is_empty()) {
            match cur {
                Node::Subtree(children) => {
                    cur = children.get(segment)?;
                }
                _ => return None,
            }
        }
        Some(cur)
    }

    /// Resolve a scalar `values:` parameter at the start of `year`.
    ///
    /// If the requested year is beyond the last declared value and the node
    /// carries an `uprating:` index, the value is projected forward by the
    /// growth of that index (see [`ParamTree::uprated_value`]).
    pub fn get_at(&self, path: &str, year: i32) -> Option<f64> {
        self.get_at_date(path, Date::start_of_year(year))
    }

    /// Resolve a scalar `values:` parameter at a specific `date`.
    pub fn get_at_date(&self, path: &str, date: Date) -> Option<f64> {
        match self.node(path)? {
            Node::Values { values, uprating } => {
                // Before the first declared entry there is nothing to resolve
                // or project: the parameter simply did not exist yet.
                if let Some(first) = values.first_date() {
                    if date < first {
                        return None;
                    }
                }
                // Strictly beyond the last declared entry: project forward by
                // uprating. Otherwise return the in-force step value.
                match values.last_date() {
                    Some(last) if date > last => {
                        self.uprate_forward(values, uprating.as_deref(), date)
                    }
                    _ => values.get_at(date),
                }
            }
            _ => None,
        }
    }

    /// Resolve a `brackets:` node's rate (or threshold) for bracket `index` at
    /// the start of `year`.
    pub fn bracket_rate_at(&self, path: &str, index: usize, year: i32) -> Option<f64> {
        self.brackets(path)?
            .get(index)?
            .rate
            .get_at(Date::start_of_year(year))
    }

    /// As [`ParamTree::bracket_rate_at`] but for the bracket threshold.
    pub fn bracket_threshold_at(&self, path: &str, index: usize, year: i32) -> Option<f64> {
        self.brackets(path)?
            .get(index)?
            .threshold
            .get_at(Date::start_of_year(year))
    }

    /// Borrow the brackets of a `brackets:` / `scales:` node.
    pub fn brackets(&self, path: &str) -> Option<&[Bracket]> {
        match self.node(path)? {
            Node::Brackets(b) => Some(b),
            _ => None,
        }
    }

    /// The value of an index-typed `values:` node at a date (used internally
    /// and exposed for tests). Index series are themselves `values:` nodes.
    pub fn index_value_at(&self, path: &str, date: Date) -> Option<f64> {
        match self.node(path)? {
            Node::Values { values, .. } => values.get_at(date),
            _ => None,
        }
    }

    /// Project a value past its last declared entry using an uprating index.
    ///
    /// The arithmetic mirrors Python: the last known value is scaled by the
    /// ratio of the index at the requested date to the index at the value's
    /// last-known date:
    ///
    /// ```text
    /// uprated = last_value * index(requested_date) / index(last_value_date)
    /// ```
    ///
    /// With no index (or an index lookup that fails) the series is held flat at
    /// its last known value — the conservative choice Python also falls back to.
    fn uprate_forward(
        &self,
        values: &ValueMap,
        uprating: Option<&str>,
        date: Date,
    ) -> Option<f64> {
        let (last_date, last_value) = values.last_known()?;
        let index_path = match uprating {
            Some(p) => p,
            None => return Some(last_value), // hold flat
        };
        let index_now = self.index_value_at(index_path, date);
        let index_base = self.index_value_at(index_path, last_date);
        match (index_now, index_base) {
            (Some(now), Some(base)) if base != 0.0 => Some(last_value * now / base),
            _ => Some(last_value), // index unavailable → hold flat
        }
    }

    /// Public helper exposing the uprating projection for a `values:` node so
    /// callers (and tests) can project a single parameter forward explicitly.
    pub fn uprated_value(&self, path: &str, date: Date) -> Option<f64> {
        match self.node(path)? {
            Node::Values { values, uprating } => {
                self.uprate_forward(values, uprating.as_deref(), date)
            }
            _ => None,
        }
    }
}

/// `serde_yaml` exposes integers and floats separately; treat either as f64.
/// Returns `None` for YAML `null` (the Python "clear this value" marker).
fn value_as_f64(v: &Value) -> Option<f64> {
    if v.is_null() {
        return None;
    }
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_u64().map(|i| i as f64))
}

/// Recursively load a directory into a [`Node::Subtree`].
fn load_dir(dir: &Path) -> anyhow::Result<Node> {
    let mut children: BTreeMap<String, Node> = BTreeMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            children.insert(name, load_dir(&path)?);
        } else if path.extension().and_then(|e| e.to_str()) == Some("yaml")
            || path.extension().and_then(|e| e.to_str()) == Some("yml")
        {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let contents = std::fs::read_to_string(&path)?;
            let node = parse_leaf(&contents).map_err(|e| {
                anyhow::anyhow!("failed to parse parameter file {}: {}", path.display(), e)
            })?;
            children.insert(stem, node);
        }
    }
    Ok(Node::Subtree(children))
}

/// Parse a single leaf YAML into a [`Node`]. Recognises `values:`, `brackets:`,
/// and `scales:`; anything else is treated as an empty `values:` node so an
/// unrecognised metadata-only file still loads without error.
fn parse_leaf(yaml_str: &str) -> anyhow::Result<Node> {
    let root: Value = serde_yaml::from_str(yaml_str)?;
    let map = match root.as_mapping() {
        Some(m) => m,
        None => anyhow::bail!("parameter file is not a YAML mapping"),
    };

    if let Some(brackets) = map
        .get(Value::String("brackets".into()))
        .or_else(|| map.get(Value::String("scales".into())))
    {
        let seq = brackets
            .as_sequence()
            .ok_or_else(|| anyhow::anyhow!("`brackets`/`scales` must be a sequence"))?;
        let mut out = Vec::with_capacity(seq.len());
        for item in seq {
            let item_map = item
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("each bracket must be a mapping"))?;
            let rate = item_map
                .get(Value::String("rate".into()))
                .and_then(ValueMap::from_yaml)
                .unwrap_or_default();
            let threshold = item_map
                .get(Value::String("threshold".into()))
                .and_then(ValueMap::from_yaml)
                .unwrap_or_default();
            out.push(Bracket { rate, threshold });
        }
        return Ok(Node::Brackets(out));
    }

    let values = map
        .get(Value::String("values".into()))
        .and_then(ValueMap::from_yaml)
        .unwrap_or_default();

    // Python declares the uprating index under `metadata.uprating`.
    let uprating = map
        .get(Value::String("metadata".into()))
        .and_then(Value::as_mapping)
        .and_then(|m| m.get(Value::String("uprating".into())))
        .and_then(Value::as_str)
        .filter(|s| *s != "self") // `self` carries no separate index series
        .map(|s| s.to_string());

    Ok(Node::Values { values, uprating })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> ParamTree {
        let dir = format!(
            "{}/tests/fixtures/parameters",
            env!("CARGO_MANIFEST_DIR")
        );
        ParamTree::from_dir(dir).expect("fixtures should load")
    }

    #[test]
    fn value_by_date_resolution() {
        let t = fixtures();
        // Latest entry on or before 2025 is the 2023 entry (0.20).
        assert_eq!(t.get_at("gov.hmrc.income_tax.rates.basic_rate", 2025), Some(0.20));
        // Before any declared entry → None.
        assert_eq!(t.get_at("gov.hmrc.income_tax.rates.basic_rate", 2000), None);
    }

    #[test]
    fn value_step_function_picks_latest_on_or_before() {
        let t = fixtures();
        // 2024 standard allowance entry applies through 2025-04-06; the
        // 2025-04-07 entry applies from then on. At start-of-2025 the in-force
        // value is still the 2024 one.
        let at_2025 = t
            .get_at_date(
                "gov.dwp.universal_credit.standard_allowance",
                Date { year: 2025, month: 1, day: 1 },
            )
            .unwrap();
        assert!((at_2025 - 393.45).abs() < 1e-9);
        let after_uprate = t
            .get_at_date(
                "gov.dwp.universal_credit.standard_allowance",
                Date { year: 2025, month: 6, day: 1 },
            )
            .unwrap();
        assert!((after_uprate - 400.14).abs() < 1e-9);
    }

    #[test]
    fn path_lookup_traverses_directories() {
        let t = fixtures();
        assert!(t.node("gov.hmrc.income_tax").is_some());
        assert!(matches!(
            t.node("gov.hmrc.income_tax.rates.basic_rate"),
            Some(Node::Values { .. })
        ));
        assert!(t.node("gov.does.not.exist").is_none());
    }

    #[test]
    fn brackets_parse_and_resolve() {
        let t = fixtures();
        let brackets = t
            .brackets("gov.revenue_scotland.lbtt.residential_rate")
            .expect("brackets node");
        assert_eq!(brackets.len(), 5);
        // Third bracket (index 2): 5% above £250,000.
        assert_eq!(
            t.bracket_rate_at("gov.revenue_scotland.lbtt.residential_rate", 2, 2020),
            Some(0.05)
        );
        assert_eq!(
            t.bracket_threshold_at("gov.revenue_scotland.lbtt.residential_rate", 2, 2020),
            Some(250000.0)
        );
        // Before the brackets start (2015) → None.
        assert_eq!(
            t.bracket_rate_at("gov.revenue_scotland.lbtt.residential_rate", 2, 2010),
            None
        );
    }

    #[test]
    fn uprating_projects_beyond_last_declared_year() {
        let t = fixtures();
        // personal_allowance is declared through 2022 (12570) and uprated by
        // gov.indices.cpi. The last-known value's date is 2022-04-06, whose
        // in-force CPI is the 2022-01-01 entry (102.5). Project to start-of-2024
        // where CPI = 120.0:  12570 * 120.0 / 102.5.
        let projected = t
            .get_at_date(
                "gov.hmrc.income_tax.personal_allowance",
                Date { year: 2024, month: 1, day: 1 },
            )
            .unwrap();
        let expected = 12570.0 * 120.0 / 102.5;
        assert!(
            (projected - expected).abs() < 1e-6,
            "got {projected}, expected {expected}"
        );
    }

    #[test]
    fn uprating_holds_flat_without_index() {
        // A node with no uprating index is held flat past its last entry.
        let node = parse_leaf(
            "values:\n  2020-01-01: 100\n  2021-01-01: 110\n",
        )
        .unwrap();
        let t = ParamTree {
            root: Node::Subtree({
                let mut m = BTreeMap::new();
                m.insert("p".to_string(), node);
                m
            }),
        };
        assert_eq!(t.get_at("p", 2030), Some(110.0));
    }

    #[test]
    fn scales_keyword_parses_as_brackets() {
        let node = parse_leaf(
            "scales:\n- rate:\n    2020-01-01: 0.1\n  threshold:\n    2020-01-01: 0\n",
        )
        .unwrap();
        assert!(matches!(node, Node::Brackets(ref b) if b.len() == 1));
    }

    #[test]
    fn null_value_clears_within_window() {
        let mut vm = ValueMap::default();
        let node = parse_leaf(
            "values:\n  2020-01-01: 5\n  2021-01-01: null\n  2022-01-01: 7\n",
        )
        .unwrap();
        if let Node::Values { values, .. } = node {
            vm = values;
        }
        assert_eq!(vm.get_at(Date { year: 2020, month: 6, day: 1 }), Some(5.0));
        // Inside the null window the value is cleared.
        assert_eq!(vm.get_at(Date { year: 2021, month: 6, day: 1 }), None);
        assert_eq!(vm.get_at(Date { year: 2022, month: 6, day: 1 }), Some(7.0));
    }
}
