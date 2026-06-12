//! Library crate: the full simulation engine plus the reusable scoring
//! pipeline in [`run`]. The CLI binary (`main.rs`) and the python bindings
//! (`python`, behind the `python` feature) are thin layers over this.

pub mod data;
pub mod engine;
pub mod parameters;
pub mod reforms;
pub mod run;
pub mod variables;

#[cfg(feature = "python")]
mod python;
