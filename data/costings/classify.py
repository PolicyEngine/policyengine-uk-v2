"""Classify costings measures: policy area, model coverage, and incidence.

`policy_area` and `incidence` are rule-based (ordered regex rules; first
match wins). `incidence` is statutory (who directly pays or receives):
households / firms / mixed / public sector & other.

`in_model` is labelled MANUALLY, measure by measure, against the actual
parameters and variables of policyengine-uk-compiled (parameters/2025_26.yaml,
src/variables/):
  yes     — the specific lever the measure changes is a model parameter
            (e.g. UC taper_rate, income tax personal_allowance, SDLT bands,
            fuel duty rate per litre, CGT annual exempt amount)
  partial — the instrument is modelled but this lever or population is not
            separately representable (e.g. PIP eligibility criteria when only
            PIP amounts are parameterised; draught relief when alcohol duty is
            a single effective rate; the employer NICs package that bundles a
            modelled rate rise with the unmodelled Employment Allowance)
  no      — the instrument is outside the model entirely (corporation tax,
            business rates, VAT scope changes, compliance yield, one-off
            payments, admin/operational measures)

The manual labels live in IN_MODEL below as (event_key, title substring,
label); anything not listed is 'no'. Substrings are matched case-insensitively
and each must match exactly one measure within its event.
"""

import json
import re
from pathlib import Path

HERE = Path(__file__).parent

# (pattern, policy_area, in_model, incidence) — first match on title wins;
# patterns are tried against the title, then title + description.
RULES = [
    # compliance / avoidance / admin cross-cutting — before tax-head rules
    (r"avoidance|evasion|compliance|non-compliance|disguised remuneration|hidden economy|umbrella compan|tax gap|debt collection|debt recovery|hmrc.*(resource|capacity|staff)|penalt|offshore|promoters|fraud|error|informants|making tax digital|mtd|tax adviser|conditionality|security deposit|insolvency|phoenixism|electronic sales suppression|self assessment forms|cryptoasset|off.?payroll|ir35|uncertain tax treatment|construction industry scheme|coding.?out|interest rate.*unpaid tax|debt management|oecd model rules|loophole|non-derecognition", "Compliance and administration", "mixed"),
    # income tax
    (r"personal allowance|higher rate threshold|additional rate|income tax.*(rate|threshold|allowance|band)|dividend (allowance|tax)|savings (rate|allowance)|marriage allowance|starting rate for savings|basic rate limit|personal tax.*threshold", "Income tax", "households"),
    (r"salary sacrifice", "Income tax and NICs", "households"),
    (r"pension.*(tax|relief|allowance|lifetime|annual allowance)|lifetime allowance|money purchase annual allowance", "Pensions tax", "households"),
    (r"income tax|itepa|benefits in kind|company car tax|car fuel benefit|van benefit|employee expenses|termination payments|image rights|\bisa\b|lifetime isa|help to save|venture capital|eis\b|seis|vct|emi\b|enterprise management|share schemes|employee shareholder|rent.?a.?room|trading and property income|cash basis|social investment tax relief|gift aid|qualifying care relief|charitable relief|employee car ownership|employer-provided cycles", "Income tax reliefs and savings", "households"),
    # NICs
    (r"employment allowance", "Employer NICs", "firms"),
    (r"employer.*national insurance|secondary threshold|employer nics", "Employer NICs", "firms"),
    (r"national insurance|nics|class [124]", "NICs", "households"),
    # corporation tax and business
    (r"corporation tax|\bct\b|r&d|research and development|patent box|capital allowance|annual investment allowance|full expensing|super.?deduction|structures and buildings|loss relief|interest restriction|hybrid|controlled foreign|transfer pricing|diverted profits|digital services tax|bank (levy|surcharge|corporation)|surcharge on banking|intangible|creative (industr|sector)|film|high.?end tv|video games|theatre|orchestra|museums|audio.?visual|freeport|investment zone|advance corporation tax|residential property developer|electricity generator levy|energy profits levy|oil and gas|petroleum|ring fence|decommissioning|first year allowance|listing relief|stamp duty reserve|stamp taxes on shares|withholding tax|royalt|pillar 2|global minimum|loss carry back|group relief|derivatives|re-insurance|corporate capital loss", "Business taxes", "firms"),
    (r"business rates|multiplier|small business rate|transitional relief", "Business rates", "firms"),
    (r"apprenticeship levy|immigration skills|soft drinks industry|plastic packaging|aggregates|landfill|climate change (levy|agreement)|carbon|emissions trading|\bets\b|energy intensive|customs|import duty|tariff|anti.?money laundering|economic crime|building safety levy|deposit return|extended producer", "Business and environmental levies", "firms"),
    (r"insurance premium tax|ipt", "Insurance premium tax", "mixed"),
    # consumption taxes
    (r"\bvat\b.*(rate|threshold)|value added tax.*(rate|threshold)", "VAT", "mixed"),
    (r"\bvat\b|value added tax", "VAT", "mixed"),
    (r"fuel duty", "Fuel duty", "mixed"),
    (r"alcohol duty.*(uprat|freeze|cut|rpi|rates)|(beer|cider|wine|spirits) dut(y|ies).*(freeze|cut|uprat)", "Alcohol duty", "households"),
    (r"alcohol|beer|cider|wine|spirits|draught", "Alcohol duty", "households"),
    (r"vaping", "Tobacco and vaping duty", "households"),
    (r"tobacco duty.*(escalator|rpi|uprat|rates)", "Tobacco and vaping duty", "households"),
    (r"tobacco|cigarette|hand.?rolling|heated tobacco|minimum excise", "Tobacco and vaping duty", "households"),
    (r"gambling|gaming|bingo|lottery|betting|machine games", "Gambling duties", "firms"),
    (r"air passenger duty|apd", "Air passenger duty", "households"),
    (r"vehicle excise duty|\bved\b|hgv|heavy goods vehicle|road user levy|company car|motorhome|evd|mileage", "Vehicle taxes", "mixed"),
    # property and wealth
    (r"stamp duty land tax|sdlt|annual tax on enveloped|ated", "Property transaction taxes", "mixed"),
    (r"capital gains tax|cgt|entrepreneurs.? relief|business asset disposal|investors.? relief|carried interest|incorporation relief", "Capital gains tax", "households"),
    (r"inheritance tax|iht|non.?dom|domicile|foreign income and gains|remittance", "Inheritance tax and non-doms", "households"),
    (r"council tax", "Council tax", "households"),
    # welfare and pensions spending
    (r"universal credit|\buc\b|tax credits|working tax credit|child tax credit|two.?child limit|minimum income floor|taper|work allowance", "Universal credit and tax credits", "households"),
    (r"personal independen(ce|t)|pip\b|disability|attendance allowance|dla|carer|esa|employment and support|work capability|incapacity|limited capability", "Disability and carer benefits", "households"),
    (r"state pension|pension credit|triple lock|double lock|winter fuel", "State pension and pensioner benefits", "households"),
    (r"housing benefit|local housing allowance|lha|social rent|supported housing|temporary accommodation|pay to stay", "Housing support", "households"),
    (r"child benefit|hicbc|high income child benefit", "Child benefit", "households"),
    (r"benefit cap|benefit freeze|uprat|benefits? .*(uprate|freeze)|jobseeker|income support|bereavement|maternity|statutory sick pay|neonatal|welfare|cost of living payment|household support fund|free school meals|healthy start|holiday activities|childcare vouchers|social security coordination|administrative earnings|industrial injuries", "Other welfare", "households"),
    (r"national living wage|national minimum wage", "Minimum wage", "households"),
    # public sector, spending and other
    (r"student loans|doctoral|masters|tuition|maintenance|lifelong learning", "Student finance", "households"),
    (r"public service pensions|nhs pensions|teachers.? pension|goodwin|discount rate|public works loan|pwlb|land registry|coal authority|crown estate|sovereign grant|bank of england|reserves|nuclear|network rail|housing associations|local government|local authorit|mayoral|borrowing (cap|powers|limit)|debt cap|bbc|devolved|scottish government|barnett|competition and markets|uk export finance|mortgage guarantee|breathing space|compensation payments|infected blood|post office|horizon|home office fees|visa|immigration", "Public sector and other", "public sector & other"),
    (r"energy (bill|price|support)|eat out to help out|furlough|job retention|self.?employment income support|seiss|kickstart|restart|traineeship|plan for jobs|warm home", "Covid and energy support", "mixed"),
    (r"grant|funding|spending|programme|investment|infrastructure|affordable homes|talking therapies|automotive industry", "Spending measures", "public sector & other"),
]


# Manual in_model labels: (event_key, lowercase title substring, label).
# Reviewed measure-by-measure against parameters/2025_26.yaml and src/variables/.
# Everything not listed is 'no'.
IN_MODEL = [
    # income tax — PA, brackets, dividend rates/allowance, savings starter band
    ("budget_2016", "personal allowance and higher rate threshold increase", "yes"),
    ("spring_budget_2017", "dividend allowance: reduce to £2,000", "yes"),
    ("budget_2018", "personal allowance and higher rate threshold: increase to £12,500", "yes"),
    ("budget_2018", "savings: maintain thresholds for adult isa allowance and starting rate", "partial"),
    ("budget_2021", "income tax: maintain personal allowance and higher rate threshold", "yes"),
    ("autumn_budget_2021", "health and social care levy introduced from april 2022", "yes"),
    ("autumn_budget_2021", "increase rates of dividend tax by 1.25%", "yes"),
    ("autumn_budget_2021", "starting rate for savings tax band: maintain at £5,000", "yes"),
    ("spring_statement_2022", "income tax: reduce basic rate from 20% to 19%", "yes"),
    ("autumn_statement_2022", "income tax and national insurance: maintain thresholds", "yes"),
    ("autumn_statement_2022", "income tax: maintaining the basic rate at 20%", "yes"),
    ("autumn_statement_2022", "income tax: reduce the additional rate threshold", "yes"),
    ("autumn_statement_2022", "income tax: reduce the dividend allowance", "yes"),
    ("spring_budget_2023", "starting rate limit for savings income: maintain at £5,000", "yes"),
    ("spring_budget_2024", "starting rate for savings: maintain at £5,000", "yes"),
    ("autumn_budget_2024", "the starting rate for savings (srs): maintain the srs at £5,000", "yes"),
    ("budget_2025", "dividend income: increase tax rates on dividend income", "yes"),
    ("budget_2025", "personal tax: maintain the personal income tax and equivalent national insurance thresholds", "yes"),
    ("budget_2025", "property income: introduce separate tax rates for property income", "partial"),
    ("budget_2025", "savings income: increase tax rates on savings income", "partial"),
    # NICs — PT/UEL/rates, class 2 (flat rate, SPT), class 4, employer rate/ST
    ("budget_2016", "self employed: abolish class 2 nics", "yes"),
    ("spring_budget_2017", "class 4 nics: increase to 10% from april 2018", "yes"),
    ("autumn_budget_2017", "one year maintain class 4 nics at 9%", "yes"),
    ("autumn_budget_2017", "delay nics bill by one year delay nics bill by one year", "yes"),
    ("budget_2018", "nics: delay nics bill by one year and maintain class 2", "yes"),
    ("budget_2020", "national insurance: increase primary threshold and lower profit limit to £9,500", "yes"),
    ("spring_statement_2022", "national insurance: increase annual primary threshold and lower profits limit to £12,570", "yes"),
    ("spring_statement_2022", "national insurance: reduce class 2 nics payments to nil", "partial"),
    ("autumn_statement_2022", "national insurance: reverse temporary 1.25pp increase", "yes"),
    ("spring_budget_2023", "national insurance contributions: impact of maintaining the lower earnings limit", "partial"),
    ("autumn_statement_2023", "1p cut to the main rate of class 4", "yes"),
    ("autumn_statement_2023", "2p cut to the main rate of class 1", "yes"),
    ("autumn_statement_2023", "abolish class 2 self-employed nics", "yes"),
    ("autumn_statement_2023", "nics: freeze the lower earnings limit and small profits threshold", "partial"),
    ("spring_budget_2024", "cut the main rate of class 1 employee nics from 10% to 8%", "yes"),
    ("spring_budget_2024", "cut the main rate of class 4 self-employed nics from 8% to 6%", "yes"),
    ("spring_budget_2024", "national insurance contributions: freeze class 2 and 3 rates", "partial"),
    # employer NICs — employer_rate, secondary_threshold (Employment Allowance not modelled)
    ("autumn_statement_2016", "national insurance contributions: align primary and secondary thresholds", "yes"),
    ("autumn_statement_2022", "national insurance: maintain the secondary threshold", "yes"),
    ("autumn_budget_2024", "employer national insurance contributions: increase rate by 1.2 ppts", "partial"),
    ("budget_2025", "national insurance: maintain the secondary threshold", "yes"),
    # UC and tax credits — standard allowances, elements, taper, work allowances, child limit; HICBC
    ("autumn_statement_2016", "universal credit: reduce taper to 63%", "yes"),
    ("spring_budget_2017", "targeted exceptions to two child limit", "partial"),
    ("budget_2018", "universal credit: £1,000 increase to work allowance", "yes"),
    ("spending_review_2020", "increase standard allowance and basic element by £20", "yes"),
    ("budget_2021", "universal credit: maintain £20 per week increase to standard allowance", "yes"),
    ("budget_2021", "shared accommodation rate (sar): accelerate introduction of further exemptions", "partial"),
    ("autumn_budget_2021", "reduce taper rate from 63p to 55p", "yes"),
    ("spring_budget_2024", "high income child benefit charge: increase income threshold to £60,000", "yes"),
    ("autumn_budget_2024", "accelerate migration of employment and support allowance claimants", "no"),
    ("spring_statement_2025", "universal credit health element: maintain at 2025- 26 rate", "partial"),
    ("spring_statement_2025", "universal credit standard allowance: increase above inflation", "yes"),
    ("budget_2025", "universal credit child element: remove the two child limit", "yes"),
    ("budget_2025", "universal credit: changes to the standard allowance and health element", "partial"),
    # other welfare
    ("autumn_statement_2022", "benefit cap levels: uprate by cpi", "yes"),
    ("budget_2016", "benefit cap: exemption for recipients of carers and guardians allowance", "partial"),
    # state pension and pension credit — new SP weekly, PC standard minimum
    ("autumn_budget_2021", "state pension and pension credit: uprate with double lock", "yes"),
    ("autumn_statement_2022", "pension credit: uprate standard minimum guarantee by cpi", "yes"),
    ("spending_review_2020", "pension credit: uprate the standard minimum guarantee", "yes"),
    # disability and carer benefits — DLA/AA/PIP amounts, carers allowance incl. earnings disregard
    ("autumn_statement_2016", "disability benefits: eligibility test change", "partial"),
    ("autumn_statement_2016", "personal independent payment: not implementing budget 2016 measure", "partial"),
    ("budget_2016", "personal independence payments: aids and appliances", "partial"),
    ("spring_budget_2023", "dwp: increase the severe disability premium transitional element", "partial"),
    ("autumn_budget_2024", "carer’s allowance: increasing the earnings limit", "yes"),
    ("spring_statement_2025", "change the pip assessment so claimants must score four points", "partial"),
    ("spring_statement_2025", "work capability assessment: restart reassessments", "partial"),
    ("budget_2025", "personal independence payment: not proceeding with spring statement 2025 reforms", "partial"),
    ("budget_2025", "child benefit: exempt 16–19-year-olds", "partial"),
    # housing support — LHA rates, HB parameters
    ("budget_2016", "local housing allowance: implement for new tenancies", "partial"),
    ("autumn_statement_2016", "local housing allowance: adjusted roll-out", "partial"),
    ("autumn_budget_2017", "targeted affordability fund: increase", "partial"),
    ("budget_2020", "housing benefit: further shared accommodation rate exemptions", "partial"),
    ("spending_review_2020", "local housing allowance (lha): increase rates to the 30th percentile", "yes"),
    ("autumn_budget_2021", "shared accommodation rate (sar): exemptions for victims", "partial"),
    ("autumn_statement_2023", "local housing allowance (lha): set to the 30th percentile", "yes"),
    ("budget_2025", "housing benefit: reduce the financial cliff edge", "partial"),
    # CGT — annual exempt amount, basic/higher rates
    ("budget_2016", "capital gains tax: reduce basic rate to 10% and main rate to 20%", "partial"),
    ("budget_2021", "capital gains tax: maintain the annual exempt amount", "yes"),
    ("autumn_statement_2022", "capital gains tax: reduce the annual exempt amount", "yes"),
    ("spring_budget_2024", "capital gains tax: cut higher rate for property from 28% to 24%", "partial"),
    ("autumn_budget_2024", "capital gains tax: increase the main rates of cgt to 18% and 24%", "partial"),
    # property transaction taxes — SDLT bands (buyer-type reliefs/surcharges not identifiable)
    ("autumn_budget_2017", "stamp duty land tax: abolish for first time buyers", "partial"),
    ("budget_2016", "stamp duty land tax on additional properties: exemptions", "partial"),
    ("budget_2018", "stamp duty land tax: extend first time buyers relief", "partial"),
    ("spending_review_2020", "stamp duty land tax: increase nil-rate band threshold to £500k", "yes"),
    ("budget_2021", "stamp duty land tax: maintain nil-rate band at £500k", "yes"),
    ("autumn_statement_2022", "stamp duty land tax: increases to nil-rate thresholds", "yes"),
    ("autumn_statement_2022", "stamp duty land tax: ending the growth plan 2022 change", "yes"),
    ("autumn_budget_2024", "increase the higher rate of additional dwelling", "partial"),
    # VAT — standard/reduced rate only; scope and registration changes not modelled
    ("spending_review_2020", "vat: temporary reduced rate for hospitality and tourism", "partial"),
    ("budget_2021", "vat: extension to reduced rate for hospitality", "partial"),
    # fuel duty — petrol/diesel rate per litre
    ("budget_2016", "fuel duty: freeze in april 2016", "yes"),
    ("autumn_statement_2016", "fuel duty: freeze in 2017-18", "yes"),
    ("autumn_budget_2017", "fuel duties: freeze for 2018-19", "yes"),
    ("budget_2018", "fuel duty: freeze for 2019-20", "yes"),
    ("budget_2020", "fuel duty: freeze for 2020-21", "yes"),
    ("budget_2021", "fuel duty: one year freeze in 2021-22", "yes"),
    ("autumn_budget_2021", "fuel duty: one year freeze in 2022-23", "yes"),
    ("spring_statement_2022", "fuel duty: reduce main rates of petrol and diesel by 5p", "yes"),
    ("spring_budget_2023", "fuel duty: 12 month extension to the 5p cut", "yes"),
    ("spring_budget_2024", "fuel duty: 12 month extension to the 5p cut", "yes"),
    ("autumn_budget_2024", "fuel duty: one year extension to the 5p cut", "yes"),
    ("budget_2025", "fuel duty: cancel uprating for 2026-27", "yes"),
    # alcohol duty — single effective rate: across-the-board freezes yes, category-specific partial
    ("budget_2016", "alcohol duty: freeze for beer, spirits and cider", "partial"),
    ("autumn_budget_2017", "alcohol duties: freeze in 2018", "yes"),
    ("budget_2018", "alcohol duties: freeze spirits, beer and cider", "partial"),
    ("budget_2020", "alcohol duty: freeze all rates for 2020-21", "yes"),
    ("budget_2021", "alcohol duty: one year in 2021-22", "yes"),
    ("autumn_budget_2021", "alcohol duty: one year freeze from february 2022", "yes"),
    ("autumn_budget_2021", "alcohol duty: reform to alcohol duties", "partial"),
    ("autumn_statement_2022", "alcohol duty reform: changes to the new alcohol duty system", "partial"),
    ("spring_budget_2023", "alcohol duty: freeze rates until august 2023", "partial"),
    ("autumn_statement_2023", "alcohol duty: freeze rates until 1 august 2024", "yes"),
    ("spring_budget_2024", "alcohol duty: freeze rates until 1 february 2025", "yes"),
    ("autumn_budget_2024", "alcohol duty: increase draught relief", "partial"),
    # tobacco duty — single effective rate; all rate measures are category-weighted
    ("budget_2016", "hand-rolling tobacco: increase by rpi+5%", "partial"),
    ("autumn_budget_2017", "tobacco duty: continue escalator", "partial"),
    ("budget_2018", "tobacco duty: rpi plus 2ppt on all duties", "partial"),
    ("budget_2020", "tobacco duty: extend rpi plus 2ppt escalator", "partial"),
    ("spending_review_2020", "tobacco duty rates: rpi+2% on all categories", "partial"),
    ("autumn_budget_2021", "tobacco duty: increase hand rolling tobacco duty", "partial"),
    ("spring_budget_2023", "tobacco duty: increase duty on hand rolling tobacco", "partial"),
    ("autumn_statement_2023", "tobacco duty: increase duty on hand rolling tobacco", "partial"),
    ("spring_budget_2024", "tobacco duty: one-off increase", "partial"),
]


def classify(title, description):
    for pat, area, incidence in RULES:
        if re.search(pat, title, re.I):
            return area, incidence
    for pat, area, incidence in RULES:
        if re.search(pat, f"{title} {description}", re.I):
            return area, incidence
    return "Other", "mixed"


def main():
    db = json.loads((HERE / "extracted.json").read_text())
    from collections import Counter

    manual = {}  # (event_key, substring) -> label
    for key, sub, label in IN_MODEL:
        manual[(key, sub)] = label

    areas, in_model_c, inc_c = Counter(), Counter(), Counter()
    unclassified = []
    matched = set()
    for ev in db:
        for m in ev["measures"]:
            area, incidence = classify(m["title"], m.get("description", ""))
            title_l = " ".join(m["title"].lower().split())
            hits = [
                (sub, label)
                for (key, sub), label in manual.items()
                if key == ev["key"] and sub in title_l
            ]
            assert len(hits) <= 1, f"multiple IN_MODEL matches for {ev['key']} | {m['title']}: {hits}"
            in_model = hits[0][1] if hits else "no"
            for sub, _ in hits:
                matched.add((ev["key"], sub))
            m["policy_area"] = area
            m["in_model"] = in_model
            m["incidence"] = incidence
            areas[area] += 1
            in_model_c[in_model] += 1
            inc_c[incidence] += 1
            if area == "Other":
                unclassified.append((ev["key"], m["title"][:75]))

    unmatched = set(manual) - matched
    assert not unmatched, f"IN_MODEL entries matched no measure: {sorted(unmatched)}"

    (HERE / "extracted.json").write_text(json.dumps(db, indent=1))
    for a, n in areas.most_common():
        print(f"{n:4d}  {a}")
    print("\nin_model:", dict(in_model_c))
    print("incidence:", dict(inc_c))
    print(f"\nunclassified ({len(unclassified)}):")
    for k, t in unclassified:
        print("  ", k, "|", t)


if __name__ == "__main__":
    main()
