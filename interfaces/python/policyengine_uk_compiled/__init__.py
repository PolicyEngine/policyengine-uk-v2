"""Python wrapper for the PolicyEngine UK Rust microsimulation binary."""

from pathlib import Path as _Path
import importlib.metadata as _meta

try:
    __version__ = _meta.version("policyengine_uk_compiled")
except Exception:
    __version__ = "unknown"


def _import_banner():
    """Always print version, update reminder and guide locations so AI agents see them on import."""
    pkg = _Path(__file__).parent
    print(
        f"policyengine_uk_compiled {__version__} — "
        "check PyPI for newer versions before starting work: "
        "pip install --upgrade policyengine_uk_compiled\n"
        f"AI guides: {pkg / 'SKILL.md'} (analysis methodology, or print_skill()); "
        f"{pkg / 'CLAUDE.md'} (API reference, or print_guide())"
    )


def print_guide():
    """Print the full AI/LLM usage guide (API reference) to stdout."""
    guide = _Path(__file__).parent / "CLAUDE.md"
    print(guide.read_text())


def print_skill():
    """Print the analysis skill guide (methodology and skepticism practices) to stdout."""
    skill = _Path(__file__).parent / "SKILL.md"
    print(skill.read_text())


_import_banner()

from policyengine_uk_compiled.models import (
    SimulationConfig,
    SimulationResult,
    MicrodataResult,
    BudgetaryImpact,
    IncomeBreakdown,
    ProgramBreakdown,
    Caseloads,
    DecileImpact,
    WinnersLosers,
    HbaiIncomes,
    PovertyHeadcounts,
    IncomeTaxParams,
    NationalInsuranceParams,
    UniversalCreditParams,
    ChildBenefitParams,
    StatePensionParams,
    PensionCreditParams,
    BenefitCapParams,
    HousingBenefitParams,
    TaxCreditsParams,
    ScottishChildPaymentParams,
    StampDutyBand,
    StampDutyParams,
    CapitalGainsTaxParams,
    WealthTaxParams,
    DlaParams,
    AaParams,
    PipParams,
    LabourSupplyParams,
    Parameters,
)
from policyengine_uk_compiled.engine import (
    Simulation,
    PERSON_DEFAULTS,
    BENUNIT_DEFAULTS,
    HOUSEHOLD_DEFAULTS,
)
from policyengine_uk_compiled.structural import StructuralReform, aggregate_microdata, combine_microdata
from policyengine_uk_compiled.data import download_all, ensure_year, ensure_dataset, DATASETS, capabilities
from policyengine_uk_compiled.realterms import cpi_index_for_year, deflate, CPI_BASE_YEAR

__all__ = [
    "Simulation",
    "StructuralReform",
    "aggregate_microdata",
    "combine_microdata",
    "PERSON_DEFAULTS",
    "BENUNIT_DEFAULTS",
    "HOUSEHOLD_DEFAULTS",
    "download_all",
    "ensure_year",
    "ensure_dataset",
    "DATASETS",
    "capabilities",
    "cpi_index_for_year",
    "deflate",
    "CPI_BASE_YEAR",
    "SimulationConfig",
    "SimulationResult",
    "MicrodataResult",
    "BudgetaryImpact",
    "IncomeBreakdown",
    "ProgramBreakdown",
    "Caseloads",
    "DecileImpact",
    "WinnersLosers",
    "IncomeTaxParams",
    "NationalInsuranceParams",
    "UniversalCreditParams",
    "ChildBenefitParams",
    "StatePensionParams",
    "PensionCreditParams",
    "BenefitCapParams",
    "HousingBenefitParams",
    "TaxCreditsParams",
    "ScottishChildPaymentParams",
    "StampDutyBand",
    "StampDutyParams",
    "CapitalGainsTaxParams",
    "WealthTaxParams",
    "DlaParams",
    "AaParams",
    "PipParams",
    "LabourSupplyParams",
    "Parameters",
]
