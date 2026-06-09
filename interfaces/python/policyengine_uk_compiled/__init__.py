"""Python wrapper for the PolicyEngine UK Rust microsimulation binary."""

from pathlib import Path as _Path


def _first_import_hint():
    """On first import, print a hint about the bundled AI usage guide."""
    try:
        marker = _Path.home() / ".policyengine_uk_compiled"
        if marker.exists():
            return
        marker.write_text("")
        guide = _Path(__file__).parent / "CLAUDE.md"
        if not guide.exists():
            return
        print(
            "\n"
            "policyengine_uk_compiled: AI usage guide available.\n"
            "Run `policyengine_uk_compiled.print_guide()` to print it "
            "(useful for giving context to Claude/ChatGPT/etc).\n"
        )
    except Exception:
        pass


def print_guide():
    """Print the full AI/LLM usage guide to stdout."""
    guide = _Path(__file__).parent / "CLAUDE.md"
    print(guide.read_text())


_first_import_hint()

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
    UcMigrationRates,
    StampDutyBand,
    StampDutyParams,
    CapitalGainsTaxParams,
    WealthTaxParams,
    CouncilTaxParams,
    CouncilTaxReductionParams,
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
    "UcMigrationRates",
    "StampDutyBand",
    "StampDutyParams",
    "CapitalGainsTaxParams",
    "WealthTaxParams",
    "CouncilTaxParams",
    "CouncilTaxReductionParams",
    "DlaParams",
    "AaParams",
    "PipParams",
    "LabourSupplyParams",
    "Parameters",
]
