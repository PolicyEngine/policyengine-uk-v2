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
| `changelog.d/` | Towncrier-style changelog fragments |

## Caveats

FRS under-reports UC receipt at ~60% of actual, so UC reform costings from this model will be proportionally lower than OBR/DWP estimates. See the [Limitations](https://policyengine.github.io/policyengine-uk-rust/#limitations) page for detail.

## Licence

MIT
