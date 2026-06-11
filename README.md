# policyengine-uk-rust

High-performance UK tax-benefit microsimulation engine in Rust, with a Python wrapper ([`policyengine-uk-compiled`](https://pypi.org/project/policyengine-uk-compiled/)).

Simulates income tax, National Insurance, Universal Credit, Child Benefit, and 10+ other programmes at ~0.1 ms per household. Reforms are expressed as a JSON overlay on the baseline parameter set — no recompilation needed.

**[Documentation](https://policyengine.github.io/policyengine-uk-rust/)**

## Quick start

```python
pip install policyengine-uk-compiled
```

```python
from policyengine_uk_compiled import PolicyEngineUK

engine = PolicyEngineUK(dataset="frs", year=2025)

# Baseline
baseline = engine.run()

# Reform: reduce UC taper rate
result = engine.run(reform={"universal_credit": {"taper_rate": 0.50}})

net_cost = result["budgetary_impact"]["net_cost"] - baseline["budgetary_impact"]["net_cost"]
print(f"UC taper 55→50%: £{net_cost / 1e9:.2f}bn/yr")
```

See the [documentation site](https://policyengine.github.io/policyengine-uk-rust/) for the full Python API, CLI reference, dataset guide, and parameter documentation.

## Building from source

```
cargo build --release
cargo test
```

## Key files

| Path | Description |
|---|---|
| `parameters/YYYY_YY.yaml` | Tax and benefit parameters, one file per fiscal year |
| `LEGISLATIVE_REFERENCE.md` | Statutory citations for all parameter values |
| `interfaces/python/` | Python wrapper (`policyengine-uk-compiled`) |
| `src/engine/` | Core simulation logic |
| `src/axiom/` | Axiom rules-engine backend (statute-derived rules) |
| `changelog.d/` | Towncrier-style changelog fragments |

## Axiom backend

NI Class 1 and 4, child benefit, the pension credit guarantee credit, and universal credit run on the [axiom rules engine](https://github.com/TheAxiomFoundation/axiom-rules-engine): every `Simulation` translates the model's parameters onto statute-derived rules compiled from [rulespec-uk](https://github.com/TheAxiomFoundation/rulespec-uk), with the hand-coded formulas retained only as the verification reference for the equivalence tests.

The compiled rules live as artifact snapshots in `src/axiom/artifacts/`, each traceable to a compose spec (in `src/axiom/programs/`, or [axiom-programs](https://github.com/TheAxiomFoundation/axiom-programs) for universal credit), the rulespec-uk rules it composes, and the engine rev pinned in `Cargo.toml`. Upstream rule improvements reach this repo by regenerating the snapshots: `scripts/refresh_artifacts.sh` recomposes and recompiles all six from sibling checkouts and runs the test suite, and a weekly CI job (`artifact-drift.yml`) does the same against upstream HEAD, failing if the committed artifacts have gone stale.

## Caveats

FRS under-reports UC receipt at ~60% of actual, so UC reform costings from this model will be proportionally lower than OBR/DWP estimates. See the [Limitations](https://policyengine.github.io/policyengine-uk-rust/#limitations) page for detail.

## Licence

MIT
