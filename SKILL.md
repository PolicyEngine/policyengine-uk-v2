# policyengine-uk-rust — Caretaker Skill

## Repo overview

Rust microsimulation engine for the UK tax-benefit system, with a Python wrapper (`interfaces/python/policyengine_uk_compiled`). Simulates income tax, NI, UC, Child Benefit, and 10+ other programmes. ~0.1ms per household.

## Running simulations

Use the Python interface. The full usage guide (constructors, reform parameters, microdata, batching, MTR patterns) lives at:

```
interfaces/python/policyengine_uk_compiled/CLAUDE.md
```

Quick reference:

```python
from policyengine_uk_compiled import Simulation, Parameters, UniversalCreditParams

# Full population (FRS auto-downloaded via POLICYENGINE_UK_DATA_TOKEN)
result = Simulation(year=2025).run()

# Reform
result = Simulation(year=2025).run(policy=Parameters(
    universal_credit=UniversalCreditParams(taper_rate=0.50)
))

# Hypothetical single person
persons, benunits, households = Simulation.single_person(employment_income=30_000)
result = Simulation(year=2025, persons=persons, benunits=benunits, households=households).run()
```

`POLICYENGINE_UK_DATA_TOKEN` for FRS auto-download is in `~/10ds/10ds-atlas-microsim/.env`.

## Key CLI flags

Prefer the Python interface over the CLI. These flags exist for advanced use or debugging.

| Flag | Purpose |
|---|---|
| `--data DIR` | Clean CSV base dir (YYYY/persons.csv etc.) |
| `--year YYYY` | Fiscal year — determines which parameter set to load |
| `--policy-json '{...}'` | Inline JSON reform overlay |
| `--output json` | Machine-readable aggregate output |
| `--output-microdata-stdout` | Per-entity CSVs to stdout |
| `--export-params-json` | Dump baseline parameters |
| `--full-take-up` | Award every modelled benefit regardless of reported receipt (for hypothetical households) |
| `--stdin-data` | Read household JSON from stdin instead of FRS CSVs |

## Data

- FRS clean CSVs live in `~/.policyengine-uk-data/frs/YYYY/` (persons.csv, benunits.csv, households.csv).
- FRS 2023 is the main microdata year in use.
- **FRS under-reports UC receipt**: model gets ~3.85m UC households vs ~6.4m in reality. This is structural — `would_claim_uc` is set from FRS-reported receipt only. Any cost estimate from this model will be ~60% of an OBR/full-population estimate.

## UC simulation — key mechanics

- UC is only awarded to `on_uc_system` benunits: those who reported UC in FRS (`on_uc=True`), or under `--full-take-up`. Benunits with reported legacy benefit receipt (HB, CTC, WTC, IS) stay on the legacy system. There is no probabilistic migration.
- Taper applies to **net** earned income (gross − income tax − NI − pension contribs) above the work allowance, at `taper_rate` (baseline 55%).
- Work allowance only applies if the benunit has children or LCWRA. Rate is higher without housing costs, lower with.
- Unearned income (savings, private pension, maintenance, property, other) reduces UC pound-for-pound.

## Known model limitations

- **Caseload undercount**: FRS-reported UC receipt understates the true caseload. Policy costings will be proportionally lower than OBR/DWP estimates. The undercount is structural — routing is purely from reported receipt.
- UC earnings cutout for a single adult with no children and no rent is low by design. Higher earners with rent/children can receive UC at much higher incomes.

## Investigating a reform

Always run the reform before diagnosing. Workflow:

1. Run baseline and reform with `--output json`, check `program_breakdown.universal_credit`.
2. If surprising, switch to `--output-microdata-stdout` and merge benunits + persons to see the earner distribution, caseload, and per-household gains.
3. Sanity-check a single household manually (use `--stdin-data` with a constructed payload) and trace through the formula by hand before touching the code.

## Multi-dataset support

Five datasets are supported. FRS, LCFS, WAS, and SPI use the two-step flow: `--extract` to produce clean CSVs, then `--data` to simulate. EFRS is a composite built from FRS + WAS + LCFS and supports wealth and consumption taxes.

| Dataset | Flag | Year arg | Clean output |
|---|---|---|---|
| FRS | `--frs <tab_dir>` | FRS survey year | `data/frs/YYYY/` |
| LCFS | `--lcfs <tab_dir>` | LCFS survey year | `data/lcfs/YYYY/` |
| WAS | `--was <tab_dir>` | WAS survey year | `data/was/YYYY/` |
| SPI | `--spi <tab_dir>` | Fiscal start year | `data/spi/YYYY/` |
| EFRS | `--extract-efrs <out_dir>` | FRS survey year | `data/efrs/YYYY/` |

**EFRS (Enhanced FRS)**: imputes wealth from WAS and consumption from LCFS onto FRS microdata using random forest models. Required to model wealth taxes (wealth_tax, stamp_duty, CGT). Build with:

```bash
./policyengine-uk-rust --extract-efrs data/efrs/2023 \
  --data data/frs --year 2023 \
  --was-dir raw/was/round_7/ \
  --lcfs-dir raw/lcfs/2021/
```

Pre-built clean EFRS CSVs are on GCS at `gs://policyengine-uk-microdata/efrs/YYYY/` and downloaded automatically via `ensure_dataset("efrs", year)` in the Python wrapper. Rebuild using `python scripts/rebuild_all.py --only efrs`.

**Wealth tax note**: `capital_gains` defaults to zero on all datasets since no UK survey records realised gains. CGT reform modelling requires manually setting `capital_gains` per person via `--stdin-data` or a custom dataset.

```bash
# Extract
./policyengine-uk-rust --lcfs raw/lcfs_2023/tab/ --year 2023 --extract data/lcfs/2023/
./policyengine-uk-rust --spi  raw/spi_2022/tab/  --year 2022 --extract data/spi/2022/

# Simulate
./policyengine-uk-rust --data data/lcfs --year 2023 --output json
./policyengine-uk-rust --data data/spi  --year 2022 --output json --persons-only
```

**SPI note**: Use `--persons-only` — SPI has no household structure so benefit/decile outputs are meaningless. Output is a JSON array of per-person records with `{person_id, weight, income fields, baseline/reform tax}`.

**LCFS note**: Only 12 top-level COICOP categories are stored (p601–p612) plus petrol/diesel. Product-level codes are not kept. LCFS has ~4,200 households so aggregate totals are small but consumption patterns are correct.

**SPI file naming**: Newer files use `put{yy}{yy+1}uk.tab` (e.g. `put2223uk.tab` for 2022/23); older files use `put{YYYY}uk.tab`. The loader detects both automatically.

**UKDS data**: LCFS (SN 9468), WAS (SN 7215), SPI (SN 9422) are all under project `ecf0b3c4-29d2-4d8a-931d-0e3773a4ac0b`. Download tab zips from UKDS MCP and unzip before extracting.

## Versioning and releasing

Versions are managed via `pyproject.toml` (the source of truth) and towncrier-style changelog fragments in `changelog.d/`.

After a new version is published to PyPI, trigger a redeploy of the chat app:

```bash
gh workflow run redeploy-on-package-update.yml --repo PolicyEngine/policyengine-uk-chat
```

- **Do not** edit `CHANGELOG.md` or `Cargo.toml` versions directly — they are updated automatically by CI.
- To ship a change, drop a fragment file in `changelog.d/` with the naming convention `<slug>.<type>`:

| File suffix | Semver bump |
|---|---|
| `.fixed` | patch |
| `.changed` | patch |
| `.added` | minor |
| `.removed` | minor |
| `.breaking` | major |

Example: `changelog.d/parse-id-list-delimiters.fixed`

The content of the file is the human-readable changelog entry. CI runs `.github/bump_version.py` to infer the bump from fragment types, update `pyproject.toml`, then `publish-git-tag.sh` to tag and release.

## Building

```
cargo build --release
```

Tests: `cargo test`

## Parameter files

`parameters/YYYY_YY.yaml` — one file per fiscal year. All UC, IT, NI, benefit cap, etc. parameters. See `LEGISLATIVE_REFERENCE.md` for statutory citations.
