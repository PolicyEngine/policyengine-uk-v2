## [0.34.0] - 2026-06-01

No significant changes.


## [0.33.0] - 2026-06-01

No significant changes.


## [0.32.0] - 2026-06-01

No significant changes.


## [0.31.0] - 2026-06-01

No significant changes.


## [0.30.0] - 2026-06-01

No significant changes.


## [0.29.0] - 2026-06-01

No significant changes.


## [0.28.0] - 2026-06-01

No significant changes.


## [0.27.0] - 2026-06-01

No significant changes.


## [0.26.0] - 2026-06-01

No significant changes.


## [0.25.0] - 2026-06-01

No significant changes.


## [0.24.0] - 2026-06-01

No significant changes.


## [0.23.0] - 2026-06-01

No significant changes.


## [0.22.0] - 2026-05-29

No significant changes.


## [0.21.0] - 2026-05-22

No significant changes.


## [0.20.0] - 2026-04-15

No significant changes.


## [0.18.0] - 2026-04-10

No significant changes.


## [0.17.0] - 2026-04-10

No significant changes.


## [0.16.0] - 2026-04-08

No significant changes.


## [0.15.0] - 2026-04-08

No significant changes.


## [0.14.0] - 2026-04-08

No significant changes.


## [0.13.0] - 2026-04-08

### Added

- Add `StructuralReform(pre=..., post=...)` to the Python wrapper, enabling reforms that can't be expressed as parameter overlays. Both hooks take `(year, persons, benunits, households)` and return the modified triple, so multi-year reforms can branch by year.


## [0.12.0] - 2026-04-07

No significant changes.


## [0.11.0] - 2026-04-07

No significant changes.


## [0.10.0] - 2026-04-07

No significant changes.


## [0.9.0] - 2026-04-06

No significant changes.


## [0.8.0] - 2026-04-05

No significant changes.


## [0.7.0] - 2026-04-05

No significant changes.


## [0.6.2] - 2026-04-02

### Fixed

- Republish with correct SimulationResult model and SPI support.


## [0.6.1] - 2026-04-02

### Fixed

- Sync SimulationResult Python model with Rust output (hbai_incomes, poverty headcounts) and add --persons-only support for SPI datasets.


## [0.6.0] - 2026-04-02

### Added

- Add `dataset` parameter to `Simulation` to run simulations against SPI, LCFS, or WAS microdata. Pass `dataset="spi"`, `dataset="lcfs"`, or `dataset="was"` — data auto-downloads from GCS on first use. Also exports `ensure_dataset` and `DATASETS` from the package top-level.


## [0.5.0] - 2026-04-02

### Added

- Added HBAI income definitions (BHC/AHC, equivalised/non-equivalised means and medians) and four poverty definitions (relative/absolute × BHC/AHC) with headcounts for child, working-age, and pensioner poverty to JSON output.


## [0.4.0] - 2026-04-02

### Added

- Add GCS download support for LCFS, SPI, and WAS datasets. New ensure_dataset(dataset, year) and updated download_all() support all four datasets (frs, lcfs, spi, was) from gs://policyengine-uk-microdata.

### Fixed

- Fix Python interface CLI flags: --data-dir/--clean-frs-base/--clean-frs → --data, --frs-raw → --frs, matching the binary's current flag names. Fixes all economy-wide simulations failing with "unexpected argument '--clean-frs-base' found".


## [0.3.6] - 2026-04-02

### Changed

- Fix LCFS income columns and weights; add --uprate-to flag; generate 2026/27 clean data for FRS, LCFS, SPI, and WAS.

  LCFS loader: switch employment income to wkgrossp (weekly gross pay, well-populated), add p047p for main SE income, add p048p for investment income, and rescale weighta to UK household population (~28.3m) so weighted aggregates are correct.

  Add --uprate-to flag to --extract mode, allowing raw survey data to be extracted and uprated to a target fiscal year in one step (e.g. --frs raw/ --year 2023 --uprate-to 2026 --extract data/frs/2026/).

  Update SKILL.md to document --uprate-to and the UKDS project ID for LCFS/WAS/SPI downloads.


## [0.3.5] - 2026-03-30

### Changed

- Use variable-specific uprating indices (earnings, CPI, GDP/capita, etc.) matching policyengine-uk; fix Scottish brackets for 2025/26+; repeal two-child limit from 2026/27; upload uprated FRS data for 2024-2029 to GCS


## [0.3.4] - 2026-03-30

### Fixed

- Fix aarch64-linux wheel: build natively in manylinux container instead of cross-compiling (fixes glibc 2.39 dependency)


## [0.3.3] - 2026-03-30

### Fixed

- Fix CI: use manylinux container's bundled Python for wheel builds


## [0.3.2] - 2026-03-30

### Fixed

- Fix CI: resolve rustup/cargo toolchain detection in manylinux container builds


## [0.3.1] - 2026-03-30

### Fixed

- Fix Linux wheel builds: manylinux container for glibc compat, aarch64-linux support


## [0.3.0] - 2026-03-30

### Added

- Add aarch64-linux wheel and fix x86_64-linux glibc compatibility (build in manylinux container)


## [0.2.1] - 2026-03-30

### Fixed

- Fixed PyPI publishing pipeline: manylinux wheel tags, automated versioning trigger.


## [0.2.0] - 2026-03-30

### Added

- Initial release: compiled UK microsimulation engine with Python interface, PyPI packaging, and Modal API deployment.


# Changelog
