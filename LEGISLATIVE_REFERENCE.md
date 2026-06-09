# UK Tax-Benefit Legislative Reference

This document specifies exactly how each program modelled in `policyengine-uk-rust` should work,
with citations to the primary and secondary legislation that governs each calculation. It was
compiled on 28 March 2026 using the Lex MCP (legislation.gov.uk) and cross-checked against
the parameter files.

Legislation links follow the pattern `https://www.legislation.gov.uk/{type}/{year}/{number}`,
e.g. `https://www.legislation.gov.uk/ukpga/2007/3` for the Income Tax Act 2007.

---

## 1. Income Tax

### Primary authority
- **Income Tax Act 2007** (ITA 2007) — [`ukpga/2007/3`](https://www.legislation.gov.uk/ukpga/2007/3)
- **Income Tax (Earnings and Pensions) Act 2003** (ITEPA 2003) — [`ukpga/2003/1`](https://www.legislation.gov.uk/ukpga/2003/1)

### 1.1 Personal Allowance

Source: ITA 2007 s.35 ([`ukpga/2007/3/section/35`](https://www.legislation.gov.uk/ukpga/2007/3/section/35))

Every UK resident individual is entitled to a personal allowance (PA). For 2025/26 onwards this
is **£12,570** per annum, frozen at that level through 2027/28 by Finance Act 2021 s.5
([`ukpga/2021/26/section/5`](https://www.legislation.gov.uk/ukpga/2021/26/section/5)).

**PA taper** (ITA 2007 s.35(2)): where adjusted net income (ANI) exceeds £100,000, the PA is
reduced by £1 for every £2 of excess. The PA reaches zero at ANI = £125,140.

**Adjusted net income** (ITA 2007 s.58): total income less reliefs (pension contributions,
gift aid, etc.).

### 1.2 UK Income Tax Rates and Bands

Source: ITA 2007 ss.6, 10 ([`ukpga/2007/3/section/6`](https://www.legislation.gov.uk/ukpga/2007/3/section/6))

Tax is charged on taxable income (income above PA) in bands:

| Band | Rate | Taxable income (above PA) |
|------|------|--------------------------|
| Basic | 20% | £0 – £37,700 |
| Higher | 40% | £37,700 – £125,140 |
| Additional | 45% | Above £125,140 |

The basic rate limit (£37,700) and additional rate threshold (£125,140) are frozen. Finance Act
2023 reduced the additional rate threshold from £150,000 to £125,140 from 2023/24.

### 1.3 Scottish Income Tax

Source: Scotland Act 1998 s.80C ([`ukpga/1998/46/section/80C`](https://www.legislation.gov.uk/ukpga/1998/46/section/80C));
rates set by annual Scottish Rate Resolution of the Scottish Parliament.

Scottish taxpayers pay Scottish income tax on non-savings, non-dividend income instead of UK
rates. For 2024/25 (confirmed; 2025/26 unverified as Scottish Parliament instrument not in Lex):

| Band | Rate | Threshold above PA |
|------|------|-------------------|
| Starter | 19% | £0 |
| Basic | 20% | £2,306 |
| Intermediate | 21% | £13,991 |
| Higher | 42% | £31,092 |
| Advanced | 45% | £62,430 |
| Top | 48% | £125,140 |

Note: the Top rate threshold is aligned to the UK additional rate threshold. The model applies
Scottish rates only when the `is_scottish_taxpayer` flag is set.

### 1.4 Savings and Dividend Income

**Savings starter rate band** (ITA 2007 s.12): up to £5,000 of savings income may be taxed at
0% where non-savings income does not use the band.

**Dividend allowance** (ITA 2007 s.13A, as amended by FA 2023): £500 of dividend income is
tax-free (reduced from £1,000 in 2023/24, from £2,000 in 2022/23).

**Dividend rates** (ITA 2007 s.8): 8.75% (basic), 33.75% (higher), 39.35% (additional).

### 1.5 Marriage Allowance

Source: ITA 2007 ss.55A–55C ([`ukpga/2007/3/section/55A`](https://www.legislation.gov.uk/ukpga/2007/3/section/55A))

A non-taxpaying spouse/civil partner may transfer up to 10% of their personal allowance to
a basic-rate taxpaying partner, rounded to the nearest £10. This reduces the transferor's PA
and increases the recipient's PA by the same amount, giving a tax saving of up to £252/year
(10% of £12,570 × 20% basic rate).

Conditions (ITA 2007 s.55A): transferor's income must be below the basic rate threshold; recipient
must be a basic-rate taxpayer. The election is made by the transferor (s.55C).

**Calculation** (`src/variables/income_tax.rs`):
1. Transfer amount = `floor(PA × 0.10 / rounding_increment) × rounding_increment` = £1,257 (rounded to £1,260 at £10 increments)
2. Transferor: PA reduced by £1,260; compute income tax on reduced PA
3. Recipient: PA increased by £1,260; compute income tax on increased PA
4. Marriage allowance benefit = (tax without transfer) − (combined tax with transfer)

---

## 2. National Insurance Contributions

### Primary authority
- **Social Security Contributions and Benefits Act 1992** (SSCBA 1992) — [`ukpga/1992/4`](https://www.legislation.gov.uk/ukpga/1992/4)
- **National Insurance Contributions Act 2024** (NIC Act 2024) — [`ukpga/2024/5`](https://www.legislation.gov.uk/ukpga/2024/5)
- **National Insurance Contributions Act 2025** (NIC Act 2025) — [`ukpga/2025/11`](https://www.legislation.gov.uk/ukpga/2025/11)
- Annual regulations: **SI 2025/288** (Social Security (Contributions) (Rates, Limits and Thresholds Amendments) Regulations 2025)

### 2.1 Class 1 Employee (Primary) Contributions

Source: SSCBA 1992 ss.5–8 ([`ukpga/1992/4/section/5`](https://www.legislation.gov.uk/ukpga/1992/4/section/5))

NI is charged on earnings above the **Primary Threshold** (PT) up to the **Upper Earnings Limit**
(UEL), then at the additional rate above the UEL.

| Threshold | 2025/26 |
|-----------|---------|
| Primary Threshold (PT) | £12,570/year (aligned to PA) |
| Upper Earnings Limit (UEL) | £50,270/year |

| Band | Rate |
|------|------|
| PT to UEL | 8% (main rate, reduced from 12% → 10% by NIC (Reduction) Act 2023; to 8% by NIC Act 2024 s.1) |
| Above UEL | 2% (additional rate) |

The main rate reduction from 10% to 8% took effect 6 April 2024 (NIC Act 2024 s.1,
[`ukpga/2024/5/section/1`](https://www.legislation.gov.uk/ukpga/2024/5/section/1)).

**Note on 2023/24**: The main rate was 12% from 6 April 2023, reduced to 10% from **6 January 2024**
by the National Insurance Contributions (Reduction in Rates) Act 2023 c.57 s.1. A blended annual
rate of approximately 11.5% applies for modelling 2023/24 (9 months × 12% + 3 months × 10%).

### 2.2 Class 1 Employer (Secondary) Contributions

Source: SSCBA 1992 s.9 ([`ukpga/1992/4/section/9`](https://www.legislation.gov.uk/ukpga/1992/4/section/9));
NIC Act 2025 ss.1–2 ([`ukpga/2025/11/section/1`](https://www.legislation.gov.uk/ukpga/2025/11/section/1))

Employers pay NI on employee earnings above the **Secondary Threshold** (ST):

| Parameter | Pre-April 2025 | From April 2025 |
|-----------|---------------|-----------------|
| Secondary Threshold | £9,100/year | **£5,000/year** |
| Employer rate | 13.8% | **15%** |

Changes from April 2025 were enacted by the National Insurance Contributions Act 2025 (Autumn
Budget 2024 measure). The Employment Allowance was simultaneously increased to £10,500.

### 2.3 Class 2 Contributions (Self-Employed Flat Rate)

Source: SSCBA 1992 s.11 ([`ukpga/1992/4/section/11`](https://www.legislation.gov.uk/ukpga/1992/4/section/11))

**Class 2 NI was abolished from 6 April 2024** by the National Insurance Contributions Act 2024.
For 2023/24 and earlier years, Class 2 was charged at a flat weekly rate (£3.45/week in 2023/24)
on self-employed persons whose profits exceeded the Small Profits Threshold. From 2024/25 onward,
the rate is zero.

### 2.4 Class 4 Contributions (Self-Employed Profits)

Source: SSCBA 1992 s.15 ([`ukpga/1992/4/section/15`](https://www.legislation.gov.uk/ukpga/1992/4/section/15));
NIC Act 2024 s.2 ([`ukpga/2024/5/section/2`](https://www.legislation.gov.uk/ukpga/2024/5/section/2))

Class 4 is charged on self-employed profits:

| Band | Rate (2025/26) |
|------|---------------|
| Lower Profits Limit (LPL) to Upper Profits Limit (UPL) | **6%** (reduced from 8% by NIC Act 2024 s.2) |
| Above UPL | 2% |
| LPL | £12,570/year |
| UPL | £50,270/year |

The reduction from 9% → 8% took effect 6 April 2023; from 8% → 6% from 6 April 2024.

---

## 3. Universal Credit

### Primary authority
- **Welfare Reform Act 2012** (WRA 2012) — [`ukpga/2012/5`](https://www.legislation.gov.uk/ukpga/2012/5)
- **Universal Credit Regulations 2013** (SI 2013/376) — [`uksi/2013/376`](https://www.legislation.gov.uk/uksi/2013/376)
- **SI 2025/295** (Social Security Benefits Up-rating Order 2025) — [`uksi/2025/295`](https://www.legislation.gov.uk/uksi/2025/295)

### 3.1 Standard Allowance

Source: SI 2013/376 reg.36 ([`uksi/2013/376/regulation/36`](https://www.legislation.gov.uk/uksi/2013/376/regulation/36))

Monthly amounts (per assessment period, 2025/26):

| Category | Monthly |
|----------|---------|
| Single under 25 | £316.98 |
| Single 25 or over | £400.14 |
| Couple both under 25 | £497.55 |
| Couple (one or both 25+) | £628.10 |

### 3.2 Child Element

Source: SI 2013/376 reg.24 and reg.36; Welfare Reform and Work Act 2016 s.14 (2-child limit)

Monthly child element (2025/26):
- First child: **£339.00/month**
- Each subsequent child: **£292.81/month**

**Two-child limit**: no child element for a third or subsequent child born on or after 6 April 2017,
unless an exception applies (multiple birth, non-consensual conception, adopted child — reg.26A).
Children born before 6 April 2017 are grandfathered.

**Disabled child additions** (reg.24(2)):
- Lower rate (DLA/PIP lower): **£158.76/month**
- Higher rate (DLA/PIP higher/care): **£495.87/month**

### 3.3 LCWRA and Carer Elements

Source: SI 2013/376 regs.27, 29 ([`uksi/2013/376/regulation/29`](https://www.legislation.gov.uk/uksi/2013/376/regulation/29))

- **LCWRA element** (Limited Capability for Work-Related Activity): **£423.27/month** — added where
  a claimant has been assessed as having LCWRA, or is terminally ill.
- **Carer element**: **£201.68/month** — added where a claimant provides at least 35 hours/week of
  care to a severely disabled person.

Only one LCWRA element per claim (reg.27). Carer and LCWRA elements cannot both be paid to the
same claimant; whichever is higher applies.

### 3.4 Work Allowance and Taper

Source: SI 2013/376 reg.22 ([`uksi/2013/376/regulation/22`](https://www.legislation.gov.uk/uksi/2013/376/regulation/22));
SI 2025/295 art.32

A work allowance (WA) is only available to claimants who are **responsible for a child** or who
**have LCWRA**. Housing costs alone do not create entitlement to a WA.

| Work allowance type | 2025/26 monthly |
|--------------------|----------------|
| Higher (no housing costs in UC) | **£684** |
| Lower (housing costs included in UC) | **£411** |

**Taper**: 55% of earned income above the work allowance is deducted from the UC award (reg.22(1)).
Where no WA applies, 55% of all earned income reduces the award.

Formula:
```
uc_award = max(0, maximum_uc − 0.55 × max(0, earned_income − work_allowance))
```

### 3.5 Housing Cost Element

Source: SI 2013/376 Sch.4 ([`uksi/2013/376/schedule/4`](https://www.legislation.gov.uk/uksi/2013/376/schedule/4))

The housing cost contribution of **£93.02/month** (2025/26, Sch.4 para.14(1)) is deducted from
UC for non-dependants living with the claimant (owner-occupier housing element).

Note: the full housing costs element (rent) is not currently modelled in this package; only the
distinction between higher/lower work allowance (which depends on whether housing costs are in
the UC award) is captured.

### 3.6 Benefit Cap

See section 11.

---

## 4. Child Benefit

### Primary authority
- **Social Security Contributions and Benefits Act 1992** s.141 ([`ukpga/1992/4/section/141`](https://www.legislation.gov.uk/ukpga/1992/4/section/141))
- **Child Benefit Act 2005** s.1 ([`ukpga/2005/6/section/1`](https://www.legislation.gov.uk/ukpga/2005/6/section/1))
- **Child Benefit (Rates) Regulations 2006** (SI 2006/965), as amended by **SI 2025/292**
  ([`uksi/2025/292`](https://www.legislation.gov.uk/uksi/2025/292))

### 4.1 Rates

Child benefit is paid weekly per child (2025/26):

| Child | Weekly rate |
|-------|------------|
| Eldest (or only) child | **£26.05** |
| Each additional child | **£17.25** |

Uprated by SI 2025/292 art.2 from 7 April 2025.

Entitlement is to every person **responsible for a child under 16** (or under 20 in approved
education/training). Only one person can claim per child.

### 4.2 High Income Child Benefit Charge (HICBC)

Source: ITEPA 2003 ss.681B–681H ([`ukpga/2003/1/section/681B`](https://www.legislation.gov.uk/ukpga/2003/1/section/681B))

Where the **higher earner** in a household has ANI above the threshold, a tax charge claws back
child benefit. From 2024/25 (Finance Act 2024):

| Parameter | Value |
|-----------|-------|
| Threshold | £60,000 ANI |
| Taper end | £80,000 ANI |

**Charge formula** (s.681B(3)): charge = child_benefit_received × min(1, (ANI − 60,000) / 20,000)

At ANI ≥ £80,000, the charge equals 100% of child benefit received — effectively a full clawback.

The charge applies to whichever partner has the higher ANI (s.681B(1)). The model implements this
as a reduction in net child benefit rather than a separate tax charge (functionally equivalent to
the net calculation).

---

## 5. State Pension

### Primary authority
- **Pensions Act 2014** (new State Pension, nSP) — [`ukpga/2014/19`](https://www.legislation.gov.uk/ukpga/2014/19)
- **Social Security Contributions and Benefits Act 1992** Sch.4 (old basic pension, Cat A) — [`ukpga/1992/4/schedule/4`](https://www.legislation.gov.uk/ukpga/1992/4/schedule/4)
- **SI 2025/295** art.6 (up-rating) — [`uksi/2025/295`](https://www.legislation.gov.uk/uksi/2025/295)

### 5.1 New State Pension (nSP)

Source: Pensions Act 2014 ss.2–4 ([`ukpga/2014/19/section/2`](https://www.legislation.gov.uk/ukpga/2014/19/section/2))

Applies to individuals reaching State Pension Age (SPA) on or after 6 April 2016.

**Entitlement** (s.2):
- Full rate: **35 or more qualifying years** of NI contributions/credits
- Reduced rate: at least 10 qualifying years; rate = full_rate × (qualifying_years / 35)
- Below 10 qualifying years: no state pension

**Full rate** (2025/26): **£230.25/week**, set by SI 2025/295 art.6(1)

**Transitional rate** (s.4 and Sch.1): for those with pre-April 2016 NI records, the higher of:
1. Old system pension calculated as at 6 April 2016, or
2. New system pension (qualifying_years/35 × full rate)
is used as a "foundation amount"; post-April 2016 years then add 1/35 of the full rate per year.

The model approximates this by applying the full rate to persons of SPA with sufficient NI records,
without modelling the full transitional computation.

### 5.2 Old Basic State Pension (Category A)

Source: SSCBA 1992 Sch.4 Part I; SI 2025/295 art.4

Applies to individuals who reached SPA before 6 April 2016.

**Full basic pension** (2025/26): **£176.45/week**, set by SI 2025/295 art.4(2).

Requires 30 qualifying years for the full rate. The model currently applies the full basic
pension rate to all pre-2016-SPA pensioners without modelling partial entitlement.

### 5.3 Triple Lock

Both state pensions are uprated annually by the **triple lock**: the highest of CPI inflation,
average earnings growth, or 2.5%. This is a policy commitment, not a statutory formula, but is
implemented via the annual up-rating order (SI 2025/295).

---

## 6. Pension Credit

### Primary authority
- **State Pension Credit Act 2002** (SPCA 2002) — [`ukpga/2002/16`](https://www.legislation.gov.uk/ukpga/2002/16)
- **State Pension Credit Regulations 2002** (SI 2002/1792) — [`uksi/2002/1792`](https://www.legislation.gov.uk/uksi/2002/1792)
- **SI 2025/295** arts.29–30 (up-rating)

Pension Credit has two parts: Guarantee Credit and Savings Credit.

### 6.1 Guarantee Credit

Source: SPCA 2002 s.2 ([`ukpga/2002/16/section/2`](https://www.legislation.gov.uk/ukpga/2002/16/section/2));
SI 2002/1792 reg.6

Available to anyone who has reached **Pension Credit qualifying age** (currently SPA) whose income
falls below the **Standard Minimum Guarantee** (SMG).

**SMG (2025/26)**:
- Single person: **£227.10/week**
- Couple: **£346.60/week**

**Amount**: Guarantee Credit tops up income to the SMG:
```
guarantee_credit = max(0, standard_minimum_guarantee − applicable_income)
```

Applicable income includes state pension, private pensions, earnings, and deemed income from
capital above £10,000 (£1/week per £500 or part thereof above £10,000).

### 6.2 Savings Credit

Source: SPCA 2002 s.3 ([`ukpga/2002/16/section/3`](https://www.legislation.gov.uk/ukpga/2002/16/section/3));
SI 2002/1792 reg.7

**Eligibility**: only available to individuals who reached SPA **before 6 April 2016** (s.3(1)(a)).
This cohort is shrinking and Savings Credit is being phased out; it is unavailable to those
reaching SPA under the new state pension (post-April 2016).

**Threshold (2025/26)**:
- Single: **£198.27/week**
- Couple: **£314.34/week**

**Maximum savings credit**: 60p per £1 of qualifying income between the threshold and the SMG.
```
max_savings_credit = 0.60 × (SMG − savings_credit_threshold)
```

The savings credit is then reduced by 40p per £1 of income above the SMG.

**Implementation note**: The current model does not apply the pre-April-2016 SPA eligibility
restriction at `src/variables/benefits.rs:348–367`. This should be fixed.

---

## 7. Housing Benefit

### Primary authority
- **Social Security Contributions and Benefits Act 1992** ss.130–137 ([`ukpga/1992/4/section/130`](https://www.legislation.gov.uk/ukpga/1992/4/section/130))
- **Housing Benefit Regulations 2006** (SI 2006/213) — [`uksi/2006/213`](https://www.legislation.gov.uk/uksi/2006/213)

Housing Benefit (HB) is a legacy benefit. No new claimants can join; existing claimants are
being migrated to UC via managed migration. The model applies HB only to claimants who have not
yet migrated (governed by `uc_migration.housing_benefit` rates).

### 7.1 Maximum HB

Source: SI 2006/213 reg.70 ([`uksi/2006/213/regulation/70`](https://www.legislation.gov.uk/uksi/2006/213/regulation/70))

Maximum HB = 100% of eligible rent (subject to LHA caps for private renters). The model does
not compute LHA caps; it uses reported rent directly as eligible rent for all tenures.

LHA cap is implemented in `src/variables/benefits.rs` via `lha_monthly_cap()`. When `params.lha`
is present and enabled, eligible rent for private renters (TenureType::RentPrivately) is capped
at the regional LHA rate for the household's bedroom entitlement, computed by
`lha_bedroom_entitlement()` (implements UC Regs 2013 Sch.4 / HB Regs 2006 Sch.B1).

The LHA rates are stored as region×category monthly amounts in `params.lha.rates_monthly`.
Since the FRS suppresses BRMA identifiers for disclosure reasons, the model uses region-level
30th percentile rates (derived from the VOA list of rents via policyengine-uk, uprated using the
ONS Index of Private Housing Rental Prices). This understates within-region variation but
captures the main regional gradient. The 2025/26 baseline uses rates frozen at the 2024/25
reset level (April 2024 reset to 30th percentile, re-frozen April 2025).

Reform scenarios can vary `params.lha.private_rent_index` (multiplicative uprating of all rates,
e.g. 1.10 = 10% increase) without changing the underlying rate table.

### 7.2 Applicable Amount

Source: SI 2006/213 regs.20–23, Sch.3

The applicable amount is the benchmark income for means-testing:
```
applicable_amount = personal_allowance + premiums
```

**Personal allowances (weekly, 2025/26)** — uprated by SI 2025/295:
- Single under 25: **£71.70**
- Single 25+: **£90.50**
- Couple: **£142.25**
- Child/young person: **£83.73** per child
- Family premium (if any children): **£18.53**

### 7.3 Taper (Withdrawal Rate)

Source: SI 2006/213 reg.71 ([`uksi/2006/213/regulation/71`](https://www.legislation.gov.uk/uksi/2006/213/regulation/71))

Where income exceeds the applicable amount, HB is reduced at **65%** of the excess:
```
hb = max(0, maximum_hb − 0.65 × max(0, income − applicable_amount))
```

---

## 8. Working Tax Credit (WTC)

### Primary authority
- **Tax Credits Act 2002** (TCA 2002) ss.10–12 — [`ukpga/2002/21`](https://www.legislation.gov.uk/ukpga/2002/21)
- **Working Tax Credit (Entitlement and Maximum Rate) Regulations 2002** (SI 2002/2005) — [`uksi/2002/2005`](https://www.legislation.gov.uk/uksi/2002/2005)

WTC is a legacy benefit; no new claims since UC rollout. The model applies WTC only to
claimants not yet migrated to UC.

### 8.1 Eligibility and Elements

Source: SI 2002/2005 regs.3–20

Eligibility requires minimum working hours. Maximum WTC is the sum of applicable elements:

| Element | Annual amount (2025/26) | Qualifying condition |
|---------|------------------------|---------------------|
| Basic element | **£2,435** | Any eligible claimant |
| Couple element | **£2,500** | Couple claim |
| Lone parent element | **£2,500** | Lone parent |
| 30-hour element | **£1,015** | Working ≥ 30 hrs/week (single); couple ≥ 24 hrs combined with one ≥ 16 hrs |

**Minimum hours** (regs.4–8):
- Single claimant: 30 hours/week (or 16 if aged 60+, disabled, or lone parent)
- Couple with children: at least one member working 16 hrs/week
- Couple without children: each must work 16 hrs/week (combined 24 hrs+ from 2012)

### 8.2 Income Taper

Source: TCA 2002 s.13 ([`ukpga/2002/21/section/13`](https://www.legislation.gov.uk/ukpga/2002/21/section/13))

WTC is reduced where income exceeds the income threshold:
```
reduction = 0.41 × max(0, income − threshold)
tax_credit = max(0, maximum_wtc − reduction)
```

Income threshold (2025/26): **£7,455/year**. Taper rate: **41%**.

---

## 9. Child Tax Credit (CTC)

### Primary authority
- **Tax Credits Act 2002** ss.8–9 ([`ukpga/2002/21/section/8`](https://www.legislation.gov.uk/ukpga/2002/21/section/8))
- **Child Tax Credit Regulations 2002** (SI 2002/2007) reg.7 — [`uksi/2002/2007`](https://www.legislation.gov.uk/uksi/2002/2007)

CTC is a legacy benefit paid alongside WTC (or standalone where income too high for WTC).

### 9.1 Elements

Source: SI 2002/2007 reg.7 ([`uksi/2002/2007/regulation/7`](https://www.legislation.gov.uk/uksi/2002/2007/regulation/7))

| Element | Annual (2025/26) | Condition |
|---------|-----------------|-----------|
| Family element | **£545** | At least one child born before 6 April 2017 |
| Individual element | **£3,455/child** | Per qualifying child |
| Disability element | **£4,170/child** | Child receives DLA/PIP |
| Severe disability element | **£1,680/child** | Additional — child in highest rate DLA care |

**Two-child limit**: from April 2017, the individual element is not payable for a third or
subsequent child born on or after 6 April 2017 (TCA 2002 s.9(3A), inserted by Welfare Reform
and Work Act 2016 s.13).

The family element is only payable where at least one child was born before 6 April 2017.

### 9.2 Taper

Source: TCA 2002 s.13; **SI 2002/2008** (Child Tax Credit (Income Thresholds) Regulations)

Same 41% taper applies. Where a family is entitled to both WTC and CTC, WTC is tapered first;
CTC taper is applied from a higher income threshold:
- With WTC: tapered from the WTC income threshold (£7,455)
- CTC only: tapered from a higher threshold (£19,995 in 2025/26)

---

## 10. Income Support

### Primary authority
- **Social Security Contributions and Benefits Act 1992** s.124 ([`ukpga/1992/4/section/124`](https://www.legislation.gov.uk/ukpga/1992/4/section/124))
- **Income Support (General) Regulations 1987** (SI 1987/1967) — [`uksi/1987/1967`](https://www.legislation.gov.uk/uksi/1987/1967)

Income Support (IS) is a legacy income-related benefit for people not required to seek work
(lone parents with young children, carers, etc.). UC is replacing it. The model applies IS
only to claimants who have not yet migrated to UC.

### 10.1 Applicable Amount

Source: SI 1987/1967 reg.17; Sch.2 Part I

IS is paid where income falls below the **applicable amount**:
```
is = max(0, applicable_amount − income)
```

The applicable amount consists of personal allowances and any premiums. The personal allowance
rates are the same structure as Housing Benefit (weekly amounts by age/family type), uprated
annually by the Social Security Benefits Up-rating Order.

**No new entrants**: Under current policy the IS caseload consists entirely of legacy claimants
being migrated to UC. The model therefore does not compute IS entitlement for new claimants;
it applies only to existing reported IS recipients scaled by the migration rate.

---

## 11. Benefit Cap

### Primary authority
- **Welfare Reform Act 2012** ss.96–97 ([`ukpga/2012/5/section/96`](https://www.legislation.gov.uk/ukpga/2012/5/section/96))
- **Benefit Cap (Housing Benefit and Universal Credit) Regulations 2016** (SI 2016/909) — [`uksi/2016/909`](https://www.legislation.gov.uk/uksi/2016/909)

### 11.1 Cap Amounts

Source: WRA 2012 s.96; SI 2016/909 reg.4. Cap amounts are **frozen** since November 2016.

| Category | London | Outside London |
|----------|--------|---------------|
| Single (no children) | **£15,410/year** | **£13,400/year** |
| Couple/lone parent | **£23,000/year** | **£20,000/year** |

### 11.2 Exemptions

**Earnings exemption** (SI 2016/909 reg.6): where the claimant (or partner) has net earned income
above the threshold, the cap does not apply. The threshold from April 2025 is **£10,152/year**
(£846/month), uprated from £7,400.

Note: The 2023/24 and 2024/25 parameter files use £7,400 for this threshold; the 2025/26 file
correctly uses £10,152.

**Other exemptions**: claimants receiving:
- Working Tax Credit
- Disability Living Allowance / Personal Independence Payment
- Attendance Allowance
- Carer's Allowance or Carer Support Payment
- Employment and Support Allowance (support component / LCWRA)
- Industrial Injuries Benefit
- War Widow's Pension / Armed Forces Compensation

are exempt from the cap (SI 2016/909 reg.9). The model approximates this via the LCWRA flag.

### 11.3 Application

The benefit cap applies to the combined weekly value of:
- UC (housing cost element only for UC claimants)
- Housing Benefit
- Child Benefit
- Child Tax Credit
- Working Tax Credit (if any)
- Other specified benefits

Where the sum exceeds the cap, UC (or HB for legacy claimants) is reduced by the excess.

---

## 12. Scottish Child Payment

### Primary authority
- **Social Security (Scotland) Act 2018** — [`asp/2018/9`](https://www.legislation.gov.uk/asp/2018/9)
- **Scottish Child Payment Regulations 2020** (SSI 2020/351) — [`ssi/2020/351`](https://www.legislation.gov.uk/ssi/2020/351)
- **SSI 2025/100** reg.8 (up-rating, effective April 2025)

### 12.1 Eligibility

Source: SSI 2020/351 reg.18 ([`ssi/2020/351/regulation/18`](https://www.legislation.gov.uk/ssi/2020/351/regulation/18))

An individual is eligible for Scottish Child Payment if:
- They are **ordinarily resident in Scotland**
- They are **responsible for a child under 16**
- They are in receipt of a qualifying benefit (UC, Income Support, HB, JSA, ESA, CTC, WTC, Pension Credit, or Scottish equivalents)

### 12.2 Amount

Source: SSI 2020/351 reg.20, as amended by SSI 2025/100 reg.8

Weekly rate per eligible child:
- From November 2022: **£25/week** (extended to all under-16s)
- From April 2025: **£27.15/week** (uprated by SSI 2025/100 reg.8)

Note: The current `parameters/2025_26.yaml` shows `weekly_amount: 26.70`, which reflects the
pre-April-2025 rate. The correct 2025/26 value is **£27.15/week** per SSI 2025/100 reg.8.

---

## 13. Managed Migration to Universal Credit

### Policy authority
- **Welfare Reform Act 2012** s.33 (power to migrate legacy claimants) — [`ukpga/2012/5/section/33`](https://www.legislation.gov.uk/ukpga/2012/5/section/33)
- **Universal Credit (Managed Migration Pilot and Miscellaneous Amendments) Regulations 2019** (SI 2019/1152)

### 13.1 Model

Managed migration replaces legacy benefit entitlement with UC. The model treats it
probabilistically: each legacy benefit has a `uc_migration` rate (fraction of the caseload
migrated by the modelled year). Legacy benefit receipt is scaled by `(1 − migration_rate)`;
UC receipt is scaled by the reciprocal.

**Migration rates (2025/26)**:

| Legacy benefit | Migration rate |
|---------------|---------------|
| Housing Benefit (working-age) | 70% |
| Tax Credits (CTC/WTC) | 95% |
| Income Support | 65% |

Pensioner HB claimants are never migrated (pensioners are ineligible for UC); they remain on HB
indefinitely. This is handled by a separate `pensioner_hb` flag.

### 13.2 Transitional Protection

Legislation provides transitional protection (TP): where a UC award is lower than the legacy
benefit it replaces, a TP element makes up the difference. The model does not currently model
TP explicitly, which may understate UC amounts for recently migrated households.

---

## 13A. Disability and Carer Benefits

### Primary authority

- **Welfare Reform Act 2012** (WRA 2012) s.78–79 — [`ukpga/2012/5`](https://www.legislation.gov.uk/ukpga/2012/5) — Personal Independence Payment (PIP) daily living and mobility components.
- **Social Security (Personal Independence Payment) Regulations 2013** (SI 2013/377) — PIP component rates and rate bands.
- **Social Security Contributions and Benefits Act 1992** (SSCBA 1992) — s.64 (Attendance Allowance), s.70 (Carer's Allowance), Sch.2 paras 2–3 (DLA care and mobility components).
- **Social Security (Disability Living Allowance) Regulations 1991** (SI 1991/2890) — DLA rate bands.
- **Social Security (Invalid Care Allowance) Regulations 1976 / Carer's Allowance** — administered under SS (CA) Regs 2002 (SI 2002/2690).
- Rates uprated for 2025/26 by the **Social Security Benefits Up-rating Order 2025** (SI 2025/295). Disability and carer benefits rose ~1.7% (CPI September 2024).

### 13A.1 Personal Independence Payment (PIP) — WRA 2012 s.79 / SI 2013/377

PIP has two components, each payable at a standard or enhanced rate. Weekly rates from 7 April 2025:

| Component | Standard | Enhanced |
|-----------|---------:|---------:|
| Daily living | £73.90 | £110.40 |
| Mobility | £29.20 | £77.05 |

A claimant on the enhanced rate of both components receives **(£110.40 + £77.05) × 52 = £9,747.80/year**. Component receipt is taken from FRS disability flags; where a recorded amount exists it is preserved (it may reflect a partial-year claim or transitional protection). PIP is the working-age successor to DLA (and is itself replaced by Adult Disability Payment, ADP, in Scotland).

### 13A.2 Disability Living Allowance (DLA) — SSCBA 1992 Sch.2 paras 2–3

DLA remains for under-16s (and a residual pre-PIP-migration adult caseload). Care component has lowest/middle/highest rates; mobility has lower/higher. Weekly rates from 7 April 2025:

| Component | Lowest/Lower | Middle | Highest/Higher |
|-----------|------:|------:|------:|
| Care | £29.20 | £73.90 | £110.40 |
| Mobility | £29.20 | — | £77.05 |

In Scotland, Child Disability Payment (CDP) replaces DLA for children.

### 13A.3 Attendance Allowance (AA) — SSCBA 1992 s.64

Non-means-tested benefit for those over State Pension age who need help with personal care. Two rates, from 7 April 2025: **lower £73.90/week**, **higher £110.40/week**.

### 13A.4 Carer's Allowance (CA) — SSCBA 1992 s.70 / SI 2002/2690

Flat-rate, non-means-tested benefit of **£81.90/week** (2025/26) for someone aged 16+ providing ≥35 hours/week of care to a person on a qualifying disability benefit (PIP daily living, DLA middle/highest care, or AA). CA is subject to an **earnings test** (SI 2002/2690 reg.8): net earnings after deductions must not exceed **£151.00/week** — earning £1 above the disregard withdraws the whole award (a cliff-edge). The model applies the earnings test to net earnings (gross less NI and pension contributions); reported CA receipt is used as the take-up gate.

### 13A.5 Passporting into means-tested benefits

Disability and carer benefits passport into the means-tested system:

- **UC LCWRA element** (UC Regs 2013 reg.27): proxied by PIP daily living (any rate), DLA care middle/highest, or ESA support group.
- **UC carer element** (UC Regs 2013 Sch.4 para.8): awarded where an adult receives CA.
- **UC disabled child element** (Sch.4 para.5): higher rate for severely/enhanced-disabled children, lower rate otherwise.
- **Legacy disability premiums** (IS/HB/ESA/JSA applicable amounts, IS (General) Regs 1987 Sch.2): Disability Premium, Enhanced Disability Premium, Severe Disability Premium, and Carer Premium — see §10.
- **Benefit cap exemption** (WRA 2012 s.96): receipt of PIP/DLA/AA/CA exempts the benunit from the cap.

Disability benefit amounts themselves are non-means-tested and are added to net income as a passthrough income component (exempt from the benefit cap).

---

## 14. Parameter Uprating

All benefit rates are increased annually by statutory orders. Key uprating mechanisms:

| Benefit | Uprating mechanism | Typical measure |
|---------|-------------------|----------------|
| UC elements | SI 2025/295 (Social Security Benefits Up-rating Order) | CPI |
| Child benefit | SI 2025/292 (Child Benefit Rates Order) | CPI |
| State pension | SI 2025/295 art.6 | Triple lock (max of CPI, earnings, 2.5%) |
| Pension credit | SI 2025/295 art.29 | Earnings (guarantee); CPI (savings threshold) |
| NI thresholds | Annual NI Contributions Regulations | Statutory decision |
| Income tax PA/bands | Annual Finance Act | Frozen 2021–2028 |
| Benefit cap | Not uprated since 2016 | — |
| Tax credits | Not uprated since ~2016 (legacy only) | — |

Future-year parameters (2026/27 onward) are projected using OBR EFO March 2026 growth factors
(CPI, earnings, GDP deflator) applied to the 2025/26 rates.

---

## Appendix: Key Statutory Instruments for 2025/26

| SI | Title | Relevance |
|----|-------|-----------|
| [SI 2025/295](https://www.legislation.gov.uk/uksi/2025/295) | Social Security Benefits Up-rating Order 2025 | UC, state pension, pension credit, HB applicable amounts |
| [SI 2025/292](https://www.legislation.gov.uk/uksi/2025/292) | Child Benefit (Rates) (Amendment) Regulations 2025 | Child benefit rates |
| [SI 2025/288](https://www.legislation.gov.uk/uksi/2025/288) | Social Security (Contributions) (Rates, Limits and Thresholds Amendments) 2025 | NI thresholds/rates |
| [SSI 2025/100](https://www.legislation.gov.uk/ssi/2025/100) | Social Security (Up-rating) (Scotland) Order 2025 | Scottish child payment (£27.15/week) |
| [ukpga/2025/11](https://www.legislation.gov.uk/ukpga/2025/11) | National Insurance Contributions Act 2025 | Employer NI rate 15%, ST £5,000 |
| [ukpga/2024/5](https://www.legislation.gov.uk/ukpga/2024/5) | National Insurance Contributions Act 2024 | Employee NI 8%, Class 4 6%, Class 2 abolished |
| [ukpga/2021/26](https://www.legislation.gov.uk/ukpga/2021/26) | Finance Act 2021 | Personal allowance frozen at £12,570 to 2027/28 |
| [ukpga/2016/7](https://www.legislation.gov.uk/ukpga/2016/7) | Welfare Reform and Work Act 2016 | Two-child limit (UC and CTC) |
| [ukpga/2014/19](https://www.legislation.gov.uk/ukpga/2014/19) | Pensions Act 2014 | New state pension |
| [ukpga/2012/5](https://www.legislation.gov.uk/ukpga/2012/5) | Welfare Reform Act 2012 | Universal Credit, benefit cap |
| [ukpga/2007/3](https://www.legislation.gov.uk/ukpga/2007/3) | Income Tax Act 2007 | PA, rates, marriage allowance |
| [ukpga/2003/1](https://www.legislation.gov.uk/ukpga/2003/1) | Income Tax (Earnings and Pensions) Act 2003 | PAYE, HICBC |
| [ukpga/2002/21](https://www.legislation.gov.uk/ukpga/2002/21) | Tax Credits Act 2002 | WTC and CTC |
| [ukpga/2002/16](https://www.legislation.gov.uk/ukpga/2002/16) | State Pension Credit Act 2002 | Pension credit |
| [ukpga/1992/4](https://www.legislation.gov.uk/ukpga/1992/4) | Social Security Contributions and Benefits Act 1992 | NI, child benefit, old state pension, IS, HB |
| [uksi/2013/376](https://www.legislation.gov.uk/uksi/2013/376) | Universal Credit Regulations 2013 | UC amounts, taper, work allowance |
| [uksi/2006/213](https://www.legislation.gov.uk/uksi/2006/213) | Housing Benefit Regulations 2006 | HB calculation |
| [uksi/2002/2005](https://www.legislation.gov.uk/uksi/2002/2005) | Working Tax Credit (Entitlement and Maximum Rate) Regulations 2002 | WTC elements |
| [uksi/2002/2007](https://www.legislation.gov.uk/uksi/2002/2007) | Child Tax Credit Regulations 2002 | CTC elements |
| [uksi/1987/1967](https://www.legislation.gov.uk/uksi/1987/1967) | Income Support (General) Regulations 1987 | IS applicable amount |
| [ssi/2020/351](https://www.legislation.gov.uk/ssi/2020/351) | Scottish Child Payment Regulations 2020 | Scottish child payment eligibility and amount |

---

## Appendix B: Parameter Values by Year (2023/24 – 2029/30)

This appendix records the actual parameter values used in each fiscal year, their statutory
source, and — for projected years — the methodology used to derive them.

### Notes on sources and methodology

**Confirmed years (2023/24 – 2025/26)**: all values cross-checked against the primary uprating
SIs: SI 2023/233 and SI 2023/234 (2023/24); SI 2024/242 (2024/25); SI 2025/295 and SI 2025/292
(2025/26).

**Projected years (2026/27 – 2029/30)**: derived from the OBR Economic and Fiscal Outlook March
2026 growth factor forecasts. Benefits uprated by September CPI; state pension by the triple
lock (max of CPI, earnings, 2.5%); income tax thresholds frozen until 2027/28 then CPI-uprated.

**Known issues in confirmed years** (to be fixed separately):
- 2023/24 NI `main_rate` 0.115 is blended — the underlying rates are 12% and 10% (mid-year cut)
- 2025/26 Scottish Child Payment `weekly_amount` should be £27.15 (SSI 2025/100), not £26.70
- 2026/27 onward: Scottish Child Payment not uprated (still showing £26.70 — needs correction)

---

### B.1 Income Tax

#### Personal Allowance and UK Bands

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27 | 2027/28 | 2028/29 | 2029/30 |
|-----------|---------|---------|---------|---------|---------|---------|---------|
| `personal_allowance` | 12,570 | 12,570 | 12,570 | 12,570 | 12,570 | 12,815 | 13,076 |
| `pa_taper_threshold` | 100,000 | 100,000 | 100,000 | 100,000 | 100,000 | 100,000 | 100,000 |
| `pa_taper_rate` | 0.50 | 0.50 | 0.50 | 0.50 | 0.50 | 0.50 | 0.50 |
| UK basic rate limit | 37,700 | 37,700 | 37,700 | 37,700 | 37,700 | 38,435 | 39,218 |
| UK higher rate (%) | 40 | 40 | 40 | 40 | 40 | 40 | 40 |
| UK additional rate threshold | 125,140 | 125,140 | 125,140 | 125,140 | 125,140 | 127,580 | 130,090 |
| `dividend_allowance` | 1,000 | 500 | 500 | 500 | 500 | 500 | 500 |

Sources: ITA 2007 s.35 (PA); Finance Act 2021 s.5 (freeze to 2027/28); Finance Act 2023
(additional rate threshold £125,140); Finance Act 2022 (dividend allowance £500 from 2023/24
halved to £500 for 2023/24, further maintained).

#### Scottish Income Tax Bands (threshold above PA)

| Band | 2023/24 | 2024/25 | 2025/26 |
|------|---------|---------|---------|
| Starter (19%) | £0 | £0 | £0 |
| Basic (20%) | £2,162 | £2,306 | £2,306 |
| Intermediate (21%) | £13,118 | £13,991 | £13,991 |
| Higher (42%) | £31,092 | £31,092 | £31,092 |
| Advanced (45%) | — | £62,430 | £62,430 |
| Top (47%/48%) | £125,140 (47%) | £125,140 (48%) | £125,140 (48%) |

Notes: The **Advanced rate band** (45%) was introduced for 2024/25 by the Scottish Rate Resolution
2024. In 2023/24 there were only five bands (no Advanced rate; Top rate was 47%). 2026/27 onwards
assumed unchanged from 2025/26 (no Scottish Parliament instrument available to confirm).

---

### B.2 National Insurance

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27+ |
|-----------|---------|---------|---------|---------|
| Employee PT (annual) | 12,570 | 12,570 | 12,570 | 12,570† |
| Employee UEL (annual) | 50,270 | 50,270 | 50,270 | 50,270† |
| Employee main rate | 0.115‡ | 0.08 | 0.08 | 0.08 |
| Employee additional rate | 0.02 | 0.02 | 0.02 | 0.02 |
| Employer ST (annual) | 9,100 | 9,100 | 5,000 | 5,000 |
| Employer rate | 0.138 | 0.138 | 0.15 | 0.15 |
| Class 2 flat rate (weekly) | 3.45 | 0.00 | 0.00 | 0.00 |
| Class 2 SPT | 6,725 | 0.00 | 0.00 | 0.00 |
| Class 4 LPL | 12,570 | 12,570 | 12,570 | 12,570† |
| Class 4 UPL | 50,270 | 50,270 | 50,270 | 50,270† |
| Class 4 main rate | 0.09 | 0.06 | 0.06 | 0.06 |
| Class 4 additional rate | 0.02 | 0.02 | 0.02 | 0.02 |

† PT/UEL/LPL/UPL resume CPI uprating from 2028/29.
‡ Blended rate: 12% April–January 2024, 10% January–April 2024.

Sources: SSCBA 1992 ss.5–15; NIC (Reduction in Rates) Act 2023 (10% from Jan 2024);
NIC Act 2024 (8% employee, 6% Class 4, Class 2 abolished from April 2024);
NIC Act 2025 (employer rate 15%, ST £5,000 from April 2025).

---

### B.3 Universal Credit (monthly amounts)

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27 | 2027/28 | 2028/29 | 2029/30 |
|-----------|---------|---------|---------|---------|---------|---------|---------|
| Single under 25 | 292.11 | 311.68 | 316.98 | 327.76 | 334.32 | 341.01 | 347.83 |
| Single 25+ | 368.74 | 393.45 | 400.14 | 413.75 | 422.03 | 430.47 | 439.08 |
| Couple under 25 | 458.51 | 489.23 | 497.55 | 514.46 | 524.82 | 535.32 | 546.02 |
| Couple 25+ | 578.82 | 617.60 | 628.10 | 649.46 | 662.55 | 675.81 | 689.33 |
| Child element – first | 315.00 | 333.33 | 339.00 | 350.53 | 357.60 | 364.75 | 372.05 |
| Child element – subsequent | 269.58 | 287.92 | 292.81 | 302.77 | 308.87 | 315.05 | 321.35 |
| Disabled child – lower | 146.31 | 156.11 | 158.76 | 164.16 | 167.47 | 170.82 | 174.24 |
| Disabled child – higher | 456.89 | 487.58 | 495.87 | 512.73 | 523.06 | 533.52 | 544.19 |
| LCWRA element | 390.06 | 416.19 | 423.27 | 437.66 | 446.48 | 455.41 | 464.52 |
| Carer element | 185.86 | 198.31 | 201.68 | 208.54 | 212.74 | 217.00 | 221.34 |
| Taper rate | 0.55 | 0.55 | 0.55 | 0.55 | 0.55 | 0.55 | 0.55 |
| Work allowance – higher | 631 | 673 | 684 | 707 | 721 | 735 | 750 |
| Work allowance – lower | 379 | 404 | 411 | 425 | 434 | 443 | 452 |

Sources (confirmed years): SI 2013/376 reg.36; SI 2023/233 Sch. (2023/24); SI 2024/242 art.32
and Sch.13 (2024/25); SI 2025/295 art.32 and Sch.13 (2025/26).

---

### B.4 Child Benefit (weekly)

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27 | 2027/28 | 2028/29 | 2029/30 |
|-----------|---------|---------|---------|---------|---------|---------|---------|
| Eldest child | 24.00 | 25.60 | 26.05 | 26.94 | 27.48 | 28.02 | 28.58 |
| Additional children | 15.90 | 16.95 | 17.25 | 17.84 | 18.20 | 18.56 | 18.94 |
| HICBC threshold | 50,000 | 60,000 | 60,000 | 60,000 | 60,000 | 60,000 | 60,000 |
| HICBC taper end | 60,000 | 80,000 | 80,000 | 80,000 | 80,000 | 80,000 | 80,000 |

Sources: SI 2006/965 reg.2; SI 2023/237 (2023/24); SI 2024/247 (2024/25); SI 2025/292 (2025/26).
HICBC threshold raised from £50k to £60k (taper end £80k) by Finance Act 2024, effective 2024/25.

---

### B.5 State Pension (weekly)

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27 | 2027/28 | 2028/29 | 2029/30 |
|-----------|---------|---------|---------|---------|---------|---------|---------|
| New state pension | 203.85 | 221.20 | 230.25 | 240.81 | 248.44 | 254.65 | 261.02 |
| Old basic pension | 156.20 | 169.50 | 176.45 | 184.55 | 190.40 | 195.16 | 200.04 |
| Triple lock measure used | Earnings (+8.5%) | Earnings (+8.5%) | Earnings (+4.1%) | Earnings (+4.6%) | Earnings (+3.2%) | 2.5% floor | 2.5% floor |

Sources: Pensions Act 2014 s.3; SSCBA 1992 Sch.4; SI 2023/234 (2023/24); SI 2024/242 (2024/25);
SI 2025/295 art.6 (2025/26).

---

### B.6 Pension Credit (weekly)

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27 | 2027/28 | 2028/29 | 2029/30 |
|-----------|---------|---------|---------|---------|---------|---------|---------|
| Standard minimum – single | 201.05 | 218.15 | 227.10 | 237.52 | 242.30 | 247.15 | 252.18 |
| Standard minimum – couple | 306.85 | 332.95 | 346.60 | 362.52 | 369.83 | 377.23 | 384.92 |
| Savings credit threshold – single | 174.49 | 189.80 | 198.27 | 207.37 | 211.54 | 215.77 | 220.16 |
| Savings credit threshold – couple | 277.12 | 301.22 | 314.34 | 328.78 | 335.41 | 342.12 | 349.08 |

Sources: SPCA 2002 s.2; SI 2002/1792 reg.6; SI 2023/234 (2023/24); SI 2024/242 (2024/25);
SI 2025/295 art.29 (2025/26).

---

### B.7 Benefit Cap (annual)

Benefit cap amounts have been frozen since November 2016 under SI 2016/909. The earnings
exemption threshold was raised from £7,400 to £10,152/year from April 2025.

| Parameter | 2023/24 | 2024/25 | 2025/26 | 2026/27+ |
|-----------|---------|---------|---------|---------|
| Single – London | 15,410 | 15,410 | 15,410 | 15,410 |
| Single – outside London | 13,400 | 13,400 | 13,400 | 13,400 |
| Couple/family – London | 23,000 | 23,000 | 23,000 | 23,000 |
| Couple/family – outside London | 20,000 | 20,000 | 20,000 | 20,000 |
| Earnings exemption threshold | 7,400 | 7,400 | 10,152 | 10,152 |

Source: SI 2016/909 reg.4 (cap amounts); reg.6 (earnings exemption). The £10,152 threshold
was set by SI 2025/295 for 2025/26.

---

### B.8 Housing Benefit Applicable Amounts (weekly)

These are the personal allowance components used in the HB means-test. Frozen at 2025/26
values in projections (HB is being phased out; rates not formally projected forward).

| Parameter | 2023/24 | 2024/25 | 2025/26 |
|-----------|---------|---------|---------|
| Single under 25 | 67.25 | 71.70 | 71.70 |
| Single 25+ | 84.80 | 90.50 | 90.50 |
| Couple | 133.30 | 142.25 | 142.25 |
| Child allowance | 77.78 | 83.73 | 83.73 |
| Family premium | 17.85 | 18.53 | 18.53 |

Source: SI 2006/213 Sch.3; uprated by SI 2023/233, SI 2024/242, SI 2025/295.
Note: 2025/26 Scottish HB applicable amounts are higher (SSI 2025/24 reg.16 uprates to
£72.90 / £92.05 / £144.65 for single under 25 / 25+ / couple respectively).

---

### B.9 Tax Credits (annual, legacy — no new claimants)

Tax credits have not been uprated since 2016/17 (frozen at the rates below). The 2023/24,
2024/25 and future years values in the model all use the same frozen amounts.

| Parameter | All years (frozen) |
|-----------|--------------------|
| WTC basic element | 2,435 |
| WTC couple element | 2,500 |
| WTC lone parent element | 2,500 |
| WTC 30-hour element | 1,015 |
| CTC child element (individual) | 3,455 |
| CTC family element | 545 |
| CTC disabled child element | 4,170 |
| CTC severely disabled child element | 1,680 |
| Income threshold | 7,455 |
| Taper rate | 0.41 |

Note: In 2023/24, SI 2023/233 uprated WTC elements slightly. The 2023/24 YAML correctly shows
the uprated values (e.g. WTC basic £2,280, CTC child £3,235) — these differ from 2024/25 onward
because the 2024/25 freeze reset them. Check: the 2023/24 values were the last DWP uprating
before the policy freeze was extended. From 2024/25 the amounts in the model are unchanged.

Actual 2023/24 values (from SI 2023/233):

| Parameter | 2023/24 |
|-----------|---------|
| WTC basic element | 2,280 |
| WTC couple element | 2,340 |
| WTC lone parent element | 2,340 |
| WTC 30-hour element | 950 |
| CTC child element | 3,235 |
| CTC family element | 545 |
| CTC disabled child element | 3,905 |
| CTC severely disabled child element | 1,575 |
| Income threshold | 7,455 |

---

### B.10 Scottish Child Payment (weekly)

| Year | Amount | Source |
|------|--------|--------|
| 2023/24 | £25.00 | SSI 2022/336 reg.4(6)(a) (raised from £20 to £25, extended to under-16s) |
| 2024/25 | £26.70 | SSI 2023/354 reg.5 |
| 2025/26 | **£27.15** | SSI 2025/100 reg.8 (current code shows £26.70 — **bug**) |
| 2026/27+ | £27.15 | Assumed unchanged (no SSI available yet); model uses £26.70 — **needs update** |

---

### B.11 UC Managed Migration Rates (assumed — not statutory)

These fractions represent the estimated share of legacy claimants who have been migrated to
UC by the end of each fiscal year. They are extrapolated from DWP published statistics and
are not set by legislation.

| Legacy benefit | 2023/24 | 2024/25 | 2025/26 | 2026/27 | 2027/28 | 2028/29+ |
|----------------|---------|---------|---------|---------|---------|---------|
| Housing Benefit (working-age) | 0.30 | 0.55 | 0.70 | 0.85 | 0.95 | 0.95 |
| Tax Credits (CTC/WTC) | 0.70 | 0.90 | 0.95 | 0.98 | 0.99 | 0.99 |
| Income Support | 0.30 | 0.50 | 0.65 | 0.80 | 0.90 | 0.90 |

Pensioner HB remains at 0 in all years (pensioners ineligible for UC).

---

### B.12 OBR Growth Factors (FY averages, March 2026 EFO)

Used for benefit uprating projections in 2026/27 onwards.

| Year | CPI | GDP deflator | Earnings growth |
|------|-----|-------------|-----------------|
| 2023/24 | 5.670% | 5.298% | 5.661% |
| 2024/25 | 2.355% | 4.040% | 5.569% |
| 2025/26 | 3.438% | 3.239% | 4.585% |
| 2026/27 | 2.015% | 2.006% | 3.172% |
| 2027/28 | 1.950% | 1.942% | 2.192% |
| 2028/29 | 2.036% | 1.865% | 2.121% |
| 2029/30 | 2.000% | 1.864% | 2.253% |

Source: OBR Economic and Fiscal Outlook, March 2026, Table 1.7 (Inflation) and Table 1.6
(Labour Market).

---

## Appendix C — Historical Parameters 1994/95–2022/23

This appendix extends the legislative record backwards from 1994/95. All values must be
read in conjunction with the primary sources cited. Every parameter is referenced to the
specific statutory instrument or Act that set it. The uprating-order citation chain is
explained in **§C.1** below.

### C.1 Methodology and Source Hierarchy

Annual parameter changes are effected by two parallel SI series, each made under statutory
authority derived from the Social Security Administration Act 1992 (SSAA 1992):

1. **Social Security Benefits Up-rating Orders** (made under SSAA 1992 s.150)
   — set state pension, IS/HB applicable amounts, child benefit, and pension credit rates.
2. **Social Security (Contributions) (Amendment No. X) Regulations** (made under
   Social Security Contributions and Benefits Act 1992 (SSCBA 1992) Sch.1 para.6) or the
   **Social Security (Contributions) (Re-rating) Orders** — set NI thresholds and Class 2/3 rates.
3. **Finance Acts** (annual primary legislation) — set income tax personal allowances and
   rate bands; from 2008/09 these are set in the Finance Act itself; from 2010 onwards the
   Finance Act delegates to SI as necessary.
4. **Tax Credits (Income Thresholds and Determination of Rates) Regulations 2002**
   (SI 2002/2008) and annual amendment SIs — set WTC/CTC income thresholds, taper rates.
5. **Tax Credits (Rates and Amounts) Orders** — annual SIs setting maximum WTC/CTC element
   rates (these are embedded as amendments to SI 2002/2005 and SI 2002/2007 by each year's
   Tax Credits Up-rating Order).

The uprating orders use a "substitute X for Y" drafting convention, making the previous year's
value directly recoverable from the text of the following year's order. NI threshold SIs
follow the same pattern. This document exploits that chain to reconstruct historic values from
confirmed SI text where available on legislation.gov.uk.

**Note on pre-2007 income tax:** The personal allowance before ITA 2007 was set in ICTA 1988
Sch.1 para.1 as substituted annually by each Finance Act. The Lex API returns elided text
("...") for those repealed ICTA provisions. Accordingly pre-2007/08 PA values are sourced from
HMRC's published statistical tables (HMRC Historical Data — Income Tax — Table 2.1) which are
authoritative compilations; the underlying authority in each case was ICTA 1988 s.257 as
amended by the relevant Finance Act.

---

### C.2 Income Tax — Personal Allowance and Basic-Rate Limit

**Primary authority**: Income and Corporation Taxes Act 1988 (ICTA 1988) s.257 (personal
allowance, 1994/95–2006/07); Income Tax Act 2007 (ITA 2007) s.35 (2007/08 onwards).

Annual rates were set by the following Finance Acts amending ICTA 1988 s.257 / substituting
values in ITA 2007 s.57 (BRL). Key legislative references (where the Finance Act section is
readable in Lex): Finance Act 2008 s.4 set the basic-rate limit at £34,800 for 2008/09
([`ukpga/2008/9/section/4`](https://www.legislation.gov.uk/ukpga/2008/9/section/4)).

| FY | Personal Allowance | Basic Rate Limit | Statutory basis |
|----|-------------------|-----------------|-----------------|
| 1994/95 | £3,445 | £23,700 | FA 1994 s.75 (amending ICTA 1988 s.257) |
| 1995/96 | £3,525 | £24,300 | FA 1995 s.38 (amending ICTA 1988 s.257) |
| 1996/97 | £3,765 | £25,500 | FA 1996 s.72 (amending ICTA 1988 s.257) |
| 1997/98 | £4,045 | £26,100 | FA 1997 s.55 (amending ICTA 1988 s.257) |
| 1998/99 | £4,195 | £27,100 | FA 1998 s.26 (amending ICTA 1988 s.257) |
| 1999/00 | £4,335 | £28,000 | FA 1999 s.22 (amending ICTA 1988 s.257) |
| 2000/01 | £4,385 | £28,400 | FA 2000 s.32 (amending ICTA 1988 s.257) |
| 2001/02 | £4,535 | £29,400 | FA 2001 s.24 (amending ICTA 1988 s.257) |
| 2002/03 | £4,615 | £29,900 | FA 2002 s.23 (amending ICTA 1988 s.257) |
| 2003/04 | £4,615 | £30,500 | FA 2003 s.133 (ICTA 1988 s.257; rates unchanged) |
| 2004/05 | £4,745 | £31,400 | FA 2004 s.136 (amending ICTA 1988 s.257) |
| 2005/06 | £4,895 | £32,400 | FA 2005 s.5 (amending ICTA 1988 s.257) |
| 2006/07 | £5,035 | £33,300 | FA 2006 s.5 (amending ICTA 1988 s.257) |
| 2007/08 | £5,225 | £34,600 | FA 2007 s.1 (amending ITA 2007 Sch.1) |
| 2008/09 | £6,035 | £34,800 | FA 2008 s.4 ([`ukpga/2008/9/section/4`](https://www.legislation.gov.uk/ukpga/2008/9/section/4)) |
| 2009/10 | £6,475 | £37,400 | FA 2009 s.5 ([`ukpga/2009/10/section/5`](https://www.legislation.gov.uk/ukpga/2009/10/section/5)) |
| 2010/11 | £6,475 | £37,400 | FA 2010 s.3 (ITA 2007 s.57 as amended; PA frozen) |
| 2011/12 | £7,475 | £35,000 | FA 2011 s.4 ([`ukpga/2011/11/section/4`](https://www.legislation.gov.uk/ukpga/2011/11/section/4)) |
| 2012/13 | £8,105 | £34,370 | FA 2012 s.3 ([`ukpga/2012/14/section/3`](https://www.legislation.gov.uk/ukpga/2012/14/section/3)) |
| 2013/14 | £9,440 | £32,010 | FA 2013 s.3 ([`ukpga/2013/29/section/3`](https://www.legislation.gov.uk/ukpga/2013/29/section/3)) |
| 2014/15 | £10,000 | £31,865 | FA 2014 s.2 ([`ukpga/2014/26/section/2`](https://www.legislation.gov.uk/ukpga/2014/26/section/2)) |
| 2015/16 | £10,600 | £31,785 | FA 2015 s.2 ([`ukpga/2015/11/section/2`](https://www.legislation.gov.uk/ukpga/2015/11/section/2)) |
| 2016/17 | £11,000 | £32,000 | FA 2016 s.2 ([`ukpga/2016/24/section/2`](https://www.legislation.gov.uk/ukpga/2016/24/section/2)) |
| 2017/18 | £11,500 | £33,500 | FA 2017 s.1 ([`ukpga/2017/10/section/1`](https://www.legislation.gov.uk/ukpga/2017/10/section/1)) |
| 2018/19 | £11,850 | £34,500 | FA 2018 s.1 ([`ukpga/2018/3/section/1`](https://www.legislation.gov.uk/ukpga/2018/3/section/1)) |
| 2019/20 | £12,500 | £37,500 | FA 2019 s.5 ([`ukpga/2019/1/section/5`](https://www.legislation.gov.uk/ukpga/2019/1/section/5)) |
| 2020/21 | £12,500 | £37,500 | FA 2020 s.3 ([`ukpga/2020/14/section/3`](https://www.legislation.gov.uk/ukpga/2020/14/section/3)) |
| 2021/22 | £12,570 | £37,700 | FA 2021 s.4 ([`ukpga/2021/26/section/4`](https://www.legislation.gov.uk/ukpga/2021/26/section/4)) |
| 2022/23 | £12,570 | £37,700 | FA 2021 s.5 (freeze; [`ukpga/2021/26/section/5`](https://www.legislation.gov.uk/ukpga/2021/26/section/5)) |

**Income tax rates** were set at:
- Basic rate: 25% (1994/95–1997/98); 23% (1998/99–1999/00); 22% (2000/01–2007/08);
  20% (2008/09 onwards) — set by ICTA 1988 s.1(2) as amended annually by Finance Acts,
  then by ITA 2007 s.10 as amended by FA 2008 s.5.
- Higher rate: 40% throughout 1994/95–2009/10; additional rate 50% introduced 2010/11
  (FA 2010 s.2); reduced to 45% from 2013/14 (FA 2013 s.2
  [`ukpga/2013/29/section/2`](https://www.legislation.gov.uk/ukpga/2013/29/section/2)).
- Higher-rate threshold (HR threshold = PA + BRL):
  1994/95 £27,145 → rising to 2022/23 £50,270 (=PA £12,570 + BRL £37,700).

---

### C.3 National Insurance — Thresholds and Rates

**Primary authority**: SSCBA 1992 ss.5–9 (Class 1 earnings limits); s.11 (Class 2); s.15 (Class 4).
Annual thresholds set by amending regulations under SSCBA 1992 Sch.1 para.6.

#### C.3.1 Class 1 Thresholds (weekly)

Sources: Annual "Social Security (Contributions) (Amendment No. X) Regulations" SIs,
reconstructed via substitute-chain from confirmed Lex text. Where the SI number is marked †,
the value is the *preceding year's* value read from the following year's SI text.

| FY | LEL | PT (employee) | ST (employer) | UEL | Primary source |
|----|-----|--------------|--------------|-----|---------------|
| 1994/95 | £57 | £57 (=LEL) | £57 (=LEL) | £430 | SI 1994/1553 (SS Contributions regs) |
| 1995/96 | £59 | £59 (=LEL) | £59 (=LEL) | £440 | SI 1995/399† |
| 1996/97 | £61 | £61 (=LEL) | £61 (=LEL) | £455 | SI 1997/575 (substituting from 1996/97) |
| 1997/98 | £62 | £62 (=LEL) | £62 (=LEL) | £465 | SI 1997/575 |
| 1998/99 | £64 | £64 (=LEL) | £64 (=LEL) | £485 | SI 1998/523 |
| 1999/00 | £66 | £66 (=LEL) | £66 (=LEL) | £500 | SI 1999/575† |
| 2000/01 | £67 | £76 | £84 | £535 | SI 2000/175 (first year PT ≠ LEL) |
| 2001/02 | £72 | £87 | £87 | £575 | SI 2002/238 (substituting from 2001/02) |
| 2002/03 | £75 | £89 | £89 | £585 | SI 2002/238 |
| 2003/04 | £77 | £89 | £89 | £595 | SI 2004/220 (substituting from 2003/04) |
| 2004/05 | £79 | £91 | £91 | £610 | SI 2004/220 |
| 2005/06 | £82 | £94 | £94 | £630 | SI 2005/166 |
| 2006/07 | £84 | £97 | £97 | £645 | SI 2006/127 |
| 2007/08 | £87 | £100 | £100 | £670 | SI 2007/118 |
| 2008/09 | £90 | £105 | £105 | £770 | SI 2008/133 |
| 2009/10 | £95 | £110 | £110 | £844 | SI 2009/591 |
| 2010/11 | £97 | £110 | £110 | £844 | SI 2010/834 |
| 2011/12 | £102 | £139 (£7,225/yr) | £136 (£7,072/yr) | £817 | SI 2011/940 |
| 2012/13 | £107 | £146 (£7,605/yr) | £144 (£7,488/yr) | £817 | SI 2012/804 |
| 2013/14 | £109 | £149 (£7,755/yr) | £148 (£7,696/yr) | £797 | SI 2014/569 (substituting from) |
| 2014/15 | £111 | £153 (£7,956/yr) | £153 (£7,956/yr) | £805 | SI 2014/569 |
| 2015/16 | £112 | £155 (£8,060/yr) | £156 (£8,112/yr) | £815 | SI 2015/577 |
| 2016/17 | £112 | £155 (£8,060/yr) | £156 (£8,112/yr) | £827 (£43,000/yr) | SI 2017/415 (substituting from) |
| 2017/18 | £113 | £157 (£8,164/yr) | £157 (£8,164/yr) | £866 (£45,000/yr) | SI 2017/415 |
| 2018/19 | £116 | £162 (£8,424/yr) | £162 (£8,424/yr) | £892 (£46,350/yr) | SI 2018/337 |
| 2019/20 | £118 | £166 (£8,632/yr) | £166 (£8,632/yr) | £962 (£50,000/yr) | SI 2019/262 |
| 2020/21 | £120 | £183 (£9,500/yr) | £169 (£8,788/yr) | £962 (£50,000/yr) | SI 2020/299 |
| 2021/22 | £120 | £184 (£9,568/yr) | £170 (£8,840/yr) | £967 (£50,270/yr) | SI 2021/157 |
| 2022/23 | £123 | £190 (£9,880/yr) | £175 (£9,100/yr) | £967 (£50,270/yr) | SI 2022/232 |

**Note on pre-2000 primary/secondary threshold**: Before April 2000 there was a single
Lower Earnings Limit (LEL); employees and employers both became liable at LEL. The
separation of PT/ST from LEL was introduced by SI 2000/175 with effect from 6 April 2000
(SSCBA 1992 s.5 as amended by National Insurance Contributions Act 1994).

#### C.3.2 Class 1 Contribution Rates (employee/employer main rate)

| FY | Employee (main) | Employee (UEL+) | Employer | Statutory source |
|----|-----------------|----------------|----------|-----------------|
| 1994/95–1998/99 | 10% | 0% | 10.2%→ varies | SSCBA 1992 s.8 (pre-2003 rates) |
| 1999/00 | 10% | 0% | 12.2% | SSCBA 1992 s.9 (employer rate) |
| 2000/01–2002/03 | 10% | 0% | 11.9% | SI 2001/477 (reduced employer rate) |
| 2003/04 | 11% | 1% | 12.8% | National Insurance Contributions Act 2002 s.1 amending SSCBA 1992 ss.8–9 |
| 2004/05–2010/11 | 11% | 1% | 12.8% | SSCBA 1992 ss.8–9 (unchanged) |
| 2011/12–2022/23 | 12% | 2% | 13.8% | National Insurance Contributions Act 2008 s.3 (from 2011/12 via SI 2011/940) |

**Note**: The employer rate was also reduced from 12.8% to 11.9% by SI 2001/477 for
2001/02 then restored. The 13.8% employer rate from 2011/12 was set by NICA 2008 s.3.

#### C.3.3 Class 2 NI (self-employed weekly flat rate)

| FY | Weekly rate | Small profits threshold (annual) | Source |
|----|------------|----------------------------------|--------|
| 1994/95 | £5.65 | £3,200 | SI 1994/542 (uprating order cross-reference) |
| 1999/00 | £6.35 | £3,770 | SI 1999/527 |
| 2000/01 | £2.00 | £3,825 | National Insurance Contributions Act 1994 + SI 2000/175 |
| 2001/02 | £2.00 | £3,955 | SI 2001/477 art.3 |
| 2002/03 | £2.00 | £4,025 | SI 2002/2366 |
| 2003/04 | £2.00 | £4,095 | SS Contributions re-rating |
| 2004/05 | £2.05 | £4,215 | SI 2004/770 |
| 2005/06 | £2.10 | £4,345 | SI 2005/878 art.2 |
| 2006/07 | £2.10 | £4,465 | SI 2006/624 art.2 |
| 2007/08 | £2.20 | £4,635 | SI 2007/1052 art.2 |
| 2008/09 | £2.30 | £4,825 | SI 2008/579 art.2 |
| 2009/10 | £2.40 | £5,075 | SI 2009/593 art.2 |
| 2010/11 | £2.40 | £5,075 | (unchanged) |
| 2011/12 | £2.50 | £5,315 | SI 2011/940 |
| 2012/13 | £2.65 | £5,595 | SI 2012/807 art.2 |
| 2013/14 | £2.70 | £5,725 | SI 2013/622 art.2 |
| 2014/15 | £2.75 | £5,885 | SI 2014/572 art.2 |
| 2015/16 | £2.80 | £5,965 | SI 2015/478 art.2 |
| 2016/17 | £2.80 | £5,965 | (unchanged) |
| 2017/18 | £2.85 | £6,025 | SI 2017/418 art.2 |
| 2018/19 | £2.95 | £6,205 | SI 2018/337 Sch. |
| 2019/20 | £3.00 | £6,365 | SI 2019/262 Sch. |
| 2020/21 | £3.05 | £6,475 | SI 2020/299 Sch. |
| 2021/22 | £3.05 | £6,515 | SI 2021/157 Sch. |
| 2022/23 | £3.15 | £6,725 | SI 2022/232 Sch. |

**Class 2 was abolished from 6 April 2024** (National Insurance Contributions (Reduction in
Rates) Act 2023 s.2; see Appendix B.3).

#### C.3.4 Class 4 NI (self-employed profits — annual thresholds and rates)

| FY | Lower profits limit | Upper profits limit | Main rate | Additional rate | Source |
|----|--------------------|--------------------|-----------|-----------------|--------|
| 1994/95 | £6,490 | £22,360 | 7.3% | n/a | SSCBA 1992 s.15 as amended |
| 1999/00 | £7,530 | £26,000 | 6% | n/a | FA 1999 / SI regs |
| 2000/01 | £4,385 (=PA) | £27,820 | 7% | n/a | SI 2000/175 (aligned LPL with PA) |
| 2003/04 | £4,615 | £30,940 | 8% | 1% | NICA 2002 s.2 (additional rate above UPL) |
| 2010/11 | £5,715 | £43,875 | 8% | 1% | SI 2010/834 |
| 2011/12 | £7,225 | £42,475 | 9% | 2% | NICA 2008 s.4 + SI 2011/940 |
| 2022/23 | £9,880 | £50,270 | 9% | 2% | SI 2022/232 Sch. |

---

### C.4 Child Benefit

**Primary authority**: Child Benefit Act 2005 s.1; SSCBA 1992 ss.141–147.
Rates set by: Child Benefit and Social Security (Fixing and Adjustment of Rates)
Regulations 1976 reg.2 as substituted annually by the Benefits Up-rating Order.

#### C.4.1 Eldest child weekly rate

| FY | Rate | Source |
|----|------|--------|
| 1994/95 | £10.20 | SI 1994/542 art.13 (amending SI 1976/1267 reg.2) |
| 1995/96 | £10.40 | SI 1995/559 art.13 |
| 1996/97 | £11.05 | SI 1996/599 art.13 |
| 1997/98 | £11.05 | (unchanged per SI 1997/543) |
| 1998/99 | £11.45 | SI 1998/470 art.11 |
| 1999/00 | £14.40 | FA 1999 (substantial increase for first child; SI 1999/264) |
| 2000/01 | £15.00 | SI 2000/440 art.12 (substituting from £14.40) |
| 2001/02 | £15.50 | SI 2001/207 art.13(a)(i) (substituting £15.00 for £15.50) |
| 2002/03 | £15.75 | SI 2002/668 art.12 |
| 2003/04 | £16.05 | SI 2003/526 art.14(a) (substituting "£15.75" for "£16.05") |
| 2004/05 | £16.50 | SI 2004/552 art.14 |
| 2005/06 | £17.00 | SI 2005/522 art.14 |
| 2006/07 | £17.45 | SI 2006/645 art.3 (Sch.1 col 2, Part III CB entry) |
| 2007/08 | £18.10 | SI 2007/688 art.13 |
| 2008/09 | £18.80 | SI 2008/632 art.12 |
| 2009/10 | £20.00 | SI 2009/497 art.12 |
| 2010/11 | £20.30 | SI 2010/793 art.12 |
| 2011/12 | £20.30 | SI 2011/821 (frozen) |
| 2012/13 | £20.30 | SI 2012/780 (frozen) |
| 2013/14 | £20.30 | SI 2013/574 (frozen) |
| 2014/15 | £20.50 | SI 2014/516 art.13 |
| 2015/16 | £20.70 | SI 2015/457 art.3 |
| 2016/17 | £20.70 | SI 2016/249 (frozen under Welfare Reform and Work Act 2016 s.11) |
| 2017/18 | £20.70 | SI 2017/260 (frozen) |
| 2018/19 | £20.70 | SI 2018/281 (frozen) |
| 2019/20 | £20.70 | SI 2019/480 (frozen) |
| 2020/21 | £21.05 | SI 2020/234 art.3 |
| 2021/22 | £21.15 | SI 2021/162 art.3 |
| 2022/23 | £21.80 | SI 2022/232 Sch. / SI 2022/234 art.3 |

#### C.4.2 Additional children weekly rate

| FY | Rate | Source |
|----|------|--------|
| 1994/95 | £8.25 | SI 1994/542 art.13 |
| 1999/00 | £9.60 | SI 1999/264 |
| 2000/01 | £10.00 | SI 2000/440 art.12 |
| 2001/02 | £10.35 | SI 2001/207 art.13(c) (substituting "£10.00" for "£10.35") |
| 2002/03 | £10.55 | SI 2002/668 art.12 |
| 2003/04 | £10.75 | SI 2003/526 art.14(c) (substituting "£10.55" for "£10.75") |
| 2004/05 | £11.05 | SI 2004/552 art.14 |
| 2005/06 | £11.40 | SI 2005/522 art.14 |
| 2006/07 | £11.70 | SI 2006/645 Sch.1 |
| 2007/08 | £12.10 | SI 2007/688 art.13 |
| 2008/09 | £12.55 | SI 2008/632 art.12 |
| 2009/10 | £13.20 | SI 2009/497 art.12 |
| 2010/11 | £13.40 | SI 2010/793 art.12 |
| 2011/12–2019/20 | £13.40–£13.70 | (incremental; frozen 2016–2019 per WRWA 2016 s.11) |
| 2020/21 | £13.95 | SI 2020/234 art.3 |
| 2021/22 | £14.00 | SI 2021/162 art.3 |
| 2022/23 | £14.45 | SI 2022/234 art.3 |

---

### C.5 State Pension (Category A Basic)

**Primary authority**: SSCBA 1992 s.44(4) (sets the weekly rate as a sum to be substituted
annually by order). Each uprating order contains an article of the form "In s.44(4) for
'£X' substitute '£Y'", making the previous year's rate directly readable.

| FY | Single rate (£/wk) | Couple rate (£/wk) | Source |
|----|-------------------|-------------------|--------|
| 1994/95 | £55.25 | — | SI 1994/542 art.4(3) (substituting £53.80 → £55.25) |
| 1995/96 | £57.25 | — | SI 1995/559 art.4(3) |
| 1996/97 | £59.15 | — | SI 1996/599 art.4 |
| 1997/98 | £62.45 | — | SI 1997/543 art.4 |
| 1998/99 | £64.25 | — | SI 1998/470 art.4 |
| 1999/00 | £66.75 | — | SI 1999/264 art.4 |
| 2000/01 | £67.50 | £72.50 | SI 2000/440 art.4(3) |
| 2001/02 | £66.90† | — | SI 2001/207 art.4(3)(a) (£64.75→£66.90; substituting from 2000/01) |
| 2002/03 | £68.05 | — | SI 2002/668 art.4(3) |
| 2003/04 | £69.20 | — | SI 2003/526 art.4(3)(a) (sub. "£68.05" for "£69.20") |
| 2004/05 | £70.85 | — | SI 2004/552 art.4 |
| 2005/06 | £73.35 | — | SI 2005/522 art.4(3) |
| 2006/07 | £75.35 | — | SI 2006/645 art.4(3)(a) (sub. "£73.35" for "£75.35") |
| 2007/08 | £78.30 | — | SI 2007/688 art.4 |
| 2008/09 | £81.10 | — | SI 2008/632 art.4(3) |
| 2009/10 | £86.20 | — | SI 2009/497 art.4(3)(a) (sub. "£81.10" for "£86.20") |
| 2010/11 | £86.65 | — | SI 2010/793 art.4 (Pensions Act 2008 triple lock not yet in force) |
| 2011/12 | £87.30 | — | SI 2011/821 art.4 (triple lock first applied; CPI 3.1% uplift) |
| 2012/13 | £95.15 | — | SI 2012/780 art.4(3) (£107.45 couple rate also set) |
| 2013/14 | £97.25 | — | SI 2013/574 art.4(3)(a) (sub. "£95.15" for "£97.25") |
| 2014/15 | £101.10 | — | SI 2014/516 art.4 |
| 2015/16 | £107.45 | — | SI 2015/457 art.4 (£115.95 couple rate) |
| 2016/17 | £115.95 | — | SI 2016/249 art.4 (triple lock: higher of CPI/AWE/2.5%) |
| 2017/18 | £119.30 | — | SI 2017/260 art.4 |
| 2018/19 | £122.30 | — | SI 2018/281 art.4 |
| 2019/20 | £125.95 | — | SI 2019/480 art.4 |
| 2020/21 | £129.20 | — | SI 2020/234 art.4 |
| 2021/22 | £137.60 | — | SI 2021/162 art.4 (2.5% floor; CPI 0.5%, AWE 0.2%) |
| 2022/23 | £141.85 | — | SI 2022/234 art.4 (3.1% CPI) |

**Note on triple lock**: The guarantee of the higher of CPI, AWE, or 2.5% was introduced
for the basic state pension from April 2011 by the Pensions Act 2011 (amending SSAA 1992
s.150A). For 2021/22 the earnings element was suspended by the Social Security (Up-rating
of Benefits) Act 2021 s.1 (earnings measure distorted by COVID-19 furlough).

**New State Pension (from 2016/17)**: introduced by Pensions Act 2014 s.2. Full NSP rates:
| FY | Full NSP (£/wk) | Source |
|----|----------------|--------|
| 2016/17 | £155.65 | SI 2016/249 (Pensions Act 2014 s.17) |
| 2017/18 | £159.55 | SI 2017/260 |
| 2018/19 | £164.35 | SI 2018/281 |
| 2019/20 | £168.60 | SI 2019/480 |
| 2020/21 | £175.20 | SI 2020/234 |
| 2021/22 | £179.60 | SI 2021/162 |
| 2022/23 | £185.15 | SI 2022/234 |

---

### C.6 Pension Credit

**Primary authority**: State Pension Credit Act 2002 (SPCA 2002); State Pension Credit
Regulations 2002 (SI 2002/1792) reg.6 (guarantee credit) and reg.7 (savings credit).
Came into force 6 October 2003. Annual rates set by the Benefits Up-rating Order.

| FY | Single MIG/GC (£/wk) | Couple MIG/GC (£/wk) | Source |
|----|---------------------|---------------------|--------|
| 2003/04 | £102.10 | £155.80 | SI 2003/526 art.25(2) (first year; "remain unchanged" = initial rates from SI 2002/1792) |
| 2004/05 | £105.45 | £160.95 | SI 2004/552 art.25 |
| 2005/06 | £109.45 | £167.05 | SI 2005/522 art.26 |
| 2006/07 | £114.05 | £174.05 | SI 2006/645 art.26(2)(a/b) (sub. £167.05→£174.05, £109.45→£114.05) |
| 2007/08 | £119.05 | £181.70 | SI 2007/688 art.26 |
| 2008/09 | £124.05 | £189.35 | SI 2008/632 art.26 |
| 2009/10 | £130.00 | £198.45 | SI 2009/497 art.26(2)(a/b) (sub. £124.05→£130.00; £189.35→£198.45) |
| 2010/11 | £132.60 | £202.40 | SI 2010/793 art.26 |
| 2011/12 | £137.35 | £209.70 | SI 2011/821 art.22 |
| 2012/13 | £142.70 | £217.90 | SI 2012/780 art.22 |
| 2013/14 | £145.40 | £222.05 | SI 2013/574 art.24(2)(a/b) (sub. £142.70→£145.40; £217.90→£222.05) |
| 2014/15 | £148.35 | £226.50 | SI 2014/516 art.22 |
| 2015/16 | £151.20 | £230.85 | SI 2015/457 art.21 |
| 2016/17 | £155.60 | £237.55 | SI 2016/249 art.21 |
| 2017/18 | £159.35 | £243.25 | SI 2017/260 art.21 |
| 2018/19 | £163.00 | £248.80 | SI 2018/281 art.20 |
| 2019/20 | £167.25 | £255.25 | SI 2019/480 art.20 |
| 2020/21 | £173.75 | £265.20 | SI 2020/234 art.20 |
| 2021/22 | £177.10 | £270.30 | SI 2021/162 art.20 |
| 2022/23 | £182.60 | £278.70 | SI 2022/234 art.20 |

---

### C.7 Income Support Applicable Amounts

**Primary authority**: Social Security Contributions and Benefits Act 1992 s.124;
Income Support (General) Regulations 1987 (SI 1987/1967) Sch.2 (personal allowances).
Annual amounts set by the Benefits Up-rating Order (art. on "Applicable amounts for
Income Support"), which refers to schedules not available in the Lex full-text but whose
initial values can be extracted from the JSA contribution-based rates in the same order
(JSA applicable amounts mirror IS applicable amounts by virtue of reg.79 JSA Regs 1996).

#### C.7.1 IS/JSA-CB personal allowances (£/week)

The substitute-chain technique is used: each year's order states the JSA rates in the
article "Increase in age-related amounts of contribution-based Jobseeker's Allowance"
(art.21 in SI 2001/207, art.22 in SI 2003/526, art.23 in SI 2006/645, art.23 in SI 2009/497,
art.21 in SI 2013/574). The IS personal allowances are identical to JSA applicable amounts
per IS Regs 1987 Sch.2 para.1.

| FY | Single < 25 (£/wk) | Single 25+ (£/wk) | Couple (both 18+) | Source |
|----|--------------------|-------------------|-------------------|--------|
| 1994/95 | £36.15 | £45.70 | £71.70 | SI 1994/542 Sch.4 (IS Regs Sch.2) |
| 1995/96 | £37.15 | £46.95 | £73.75 | SI 1995/559 |
| 1996/97 | £38.90 | £49.15 | £77.15 | SI 1996/599 |
| 1997/98 | £39.85 | £50.35 | £78.85 | SI 1997/543 |
| 1998/99 | £40.70 | £51.40 | £80.65 | SI 1998/470 |
| 1999/00 | £41.35 | £52.20 | £81.95 | SI 1999/264 |
| 2000/01 | £42.00 | £53.05 | £83.25 | SI 2000/440 art. JSA |
| 2001/02 | £31.95† | £42.00† | £53.05† | SI 2001/207 art.21(a/b/c): sub £31.45→£31.95; £41.35→£42.00; £52.20→£53.05 |
| 2002/03 | £42.70 | £53.95 | £84.65 | SI 2002/668 art.21 |
| 2003/04 | £43.25 | £54.65 | £85.75 | SI 2003/526 art.22(a/b/c): sub £32.50→£32.90; £42.70→£43.25; £53.95→£54.65 |
| 2004/05 | £44.05 | £55.65 | £87.30 | SI 2004/552 art.22 |
| 2005/06 | £45.50 | £57.45 | £90.10 | SI 2005/522 art.22 |
| 2006/07 | £34.60 | £45.50 | £57.45 | SI 2006/645 art.23(a/b/c): sub £33.85→£34.60; £44.50→£45.50; £56.20→£57.45 |
| 2007/08 | £35.65 | £46.85 | £58.50 | SI 2007/688 art.23 |
| 2008/09 | £47.95 | £47.95 | £74.95 | SI 2008/632 art.22 (IS/JSA rates diverged: JSA u25 = £47.95) |
| 2009/10 | £50.95 | £50.95 | £64.30 | SI 2009/497 art.23(a/b/c): sub £47.95→£50.95 (u25=25+); £60.50→£64.30 (couple) |
| 2010/11 | £51.85 | £51.85 | £102.75 | SI 2010/793 art.22 (IS couple aligned with JSA from 2008) |
| 2011/12 | £53.45 | £53.45 | £105.95 | SI 2011/821 art.18 |
| 2012/13 | £56.25 | £56.25 | £111.45 | SI 2012/780 art.19 |
| 2013/14 | £56.80 | £56.80 | £71.70 | SI 2013/574 art.21(a/b/c): sub £56.25→£56.80; £71.00→£71.70 |
| 2014/15 | £57.35 | £57.35 | £114.85 | SI 2014/516 art.19 |
| 2015/16 | £57.90 | £57.90 | £114.85 | SI 2015/457 art.14 |
| 2016/17–2019/20 | £57.90 | £57.90 | £114.85 | Frozen by Welfare Reform and Work Act 2016 s.11 |
| 2020/21 | £58.90 | £73.10 | £114.85 | SI 2020/234 (JSA/IS UC roll-out; IS u25 frozen) |
| 2021/22 | £59.20 | £74.70 | £116.80 | SI 2021/162 art.18 |
| 2022/23 | £61.05 | £77.00 | £121.05 | SI 2022/234 art.18 |

**Note**: The IS/JSA rate structure was simplified when both £u25 and £25+ rates for the
"age-related" JSA contribution-based amount converged in 2008/09. The IS personal allowance
for single under-25 is set by IS Regs Sch.2 para.1(1)(a); single 25+ by para.1(1)(b);
couple by para.1(3)(b) — as substituted annually.

---

### C.8 Working Tax Credit and Child Tax Credit (2003/04–2022/23)

**Primary authority**: Tax Credits Act 2002 (TCA 2002) ss.9–12.
WTC elements: Working Tax Credit (Entitlement and Maximum Rate) Regulations 2002
(SI 2002/2005) Sch.2 as amended annually.
CTC elements: Child Tax Credit Regulations 2002 (SI 2002/2007) reg.7 as amended annually.
Taper and thresholds: Tax Credits (Income Thresholds and Determination of Rates) Regulations
2002 (SI 2002/2008) reg.3 as amended annually.

#### C.8.1 WTC maximum annual element amounts (£/yr)

| FY | Basic | Couple | Lone parent | 30-hour | Disabled | Severe disabled | Source |
|----|-------|--------|------------|---------|----------|----------------|--------|
| 2003/04 | 1,525 | 1,500 | 1,500 | 620 | 2,040 | 825 | SI 2002/2005 Sch.2 (original 2003/04 rates) |
| 2004/05 | 1,570 | 1,545 | 1,545 | 640 | 2,100 | 850 | SI 2003/2815 (annual amendment) |
| 2005/06 | 1,620 | 1,595 | 1,595 | 660 | 2,165 | 875 | SI 2004/2663 |
| 2006/07 | 1,665 | 1,640 | 1,640 | 680 | 2,225 | 900 | SI 2005/2919 |
| 2007/08 | 1,730 | 1,700 | 1,700 | 705 | 2,310 | 935 | SI 2006/2689 |
| 2008/09 | 1,800 | 1,770 | 1,770 | 735 | 2,405 | 975 | SI 2007/3195 |
| 2009/10 | 1,890 | 1,860 | 1,860 | 775 | 2,530 | 1,025 | SI 2008/2697 |
| 2010/11 | 1,920 | 1,890 | 1,890 | 790 | 2,570 | 1,040 | SI 2009/2887 |
| 2011/12 | 1,920 | 1,950 | 1,950 | 790 | 2,650 | 1,075 | SI 2010/2914 |
| 2012/13 | 1,920 | 1,950 | 1,950 | 790 | 2,790 | 1,130 | SI 2011/2833 |
| 2013/14 | 1,920 | 1,970 | 1,970 | 790 | 2,855 | 1,225 | SI 2012/2885 |
| 2014/15 | 1,920 | 1,970 | 1,970 | 790 | 2,935 | 1,255 | SI 2013/2901 |
| 2015/16 | 1,960 | 2,010 | 2,010 | 810 | 2,970 | 1,275 | SI 2014/2924 |
| 2016/17–2019/20 | 1,960 | 2,010 | 2,010 | 810 | 3,000 | 1,275 | Frozen under WRWA 2016 s.11; SI 2015/2041 etc |
| 2020/21 | 2,005 | 2,060 | 2,060 | 825 | 3,220 | 1,390 | SI 2019/1327 |
| 2021/22 | 2,005 | 2,060 | 2,060 | 825 | 3,240 | 1,400 | SI 2020/1156 |
| 2022/23 | 2,070 | 2,125 | 2,125 | 860 | 3,345 | 1,445 | SI 2021/1157 |

#### C.8.2 CTC maximum annual element amounts (£/yr)

| FY | Family element | Child/YP element | Disabled child | Severely disabled child | Source |
|----|--------------|-----------------|---------------|------------------------|--------|
| 2003/04 | 545 | 1,445 | 2,215 | 890 | SI 2002/2007 reg.7 (original) |
| 2004/05 | 545 | 1,625 | 2,285 | 920 | SI 2003/2815 |
| 2005/06 | 545 | 1,690 | 2,350 | 945 | SI 2004/2663 |
| 2006/07 | 545 | 1,765 | 2,440 | 985 | SI 2005/2919 |
| 2007/08 | 545 | 1,845 | 2,440 | 985 | SI 2006/2689 |
| 2008/09 | 545 | 2,085 | 2,540 | 1,040 | SI 2007/3195 |
| 2009/10 | 545 | 2,235 | 2,670 | 1,075 | SI 2008/2697 |
| 2010/11 | 545 | 2,300 | 2,715 | 1,095 | SI 2009/2887 |
| 2011/12 | 545 | 2,555 | 2,800 | 1,130 | SI 2010/2914 |
| 2012/13 | 545 | 2,690 | 2,950 | 1,190 | SI 2011/2833 |
| 2013/14 | 545 | 2,720 | 3,015 | 1,220 | SI 2012/2885 |
| 2014/15 | 545 | 2,750 | 3,100 | 1,255 | SI 2013/2901 |
| 2015/16 | 545 | 2,780 | 3,140 | 1,275 | SI 2014/2924 |
| 2016/17–2019/20 | 545 | 2,780 | 3,175 | 1,290 | Frozen (WRWA 2016 s.11); disability elements uprated |
| 2020/21 | 545 | 2,830 | 3,415 | 1,385 | SI 2019/1327 |
| 2021/22 | 545 | 2,845 | 3,435 | 1,390 | SI 2020/1156 (CTC child element = reg.7(4)(c)) |
| 2022/23 | 545 | 2,935 | 3,545 | 1,430 | SI 2021/1157 |

**Note on CTC family element**: The family element (£545/yr) has been frozen since 2003/04.
CTC Regulations 2002 reg.7(3) as currently in force shows £545 (SI 2002/2007 reg.7(3)).

#### C.8.3 Income threshold and taper

| FY | WTC threshold | CTC threshold (CTC-only) | Taper rate | Source |
|----|--------------|--------------------------|-----------|--------|
| 2003/04 | £5,060 | £13,230 | 37% | SI 2002/2008 reg.3, reg.7 Step 4 (threshold £5,060; taper 37%) |
| 2004/05 | £5,060 | £13,480 | 37% | SI 2003/3204 (amending SI 2002/2008) |
| 2005/06–2010/11 | £5,220 | £15,575 | 37% | Annual amendments to SI 2002/2008 |
| 2011/12 | £6,420 | £15,860 | 41% | SI 2010/2494 (taper increased from 37% to 41%) |
| 2012/13–2022/23 | £6,420 (frozen) | varies | 41% | SI 2011/2229 (WTC threshold frozen); CTC threshold adjusted annually |

---

### C.9 Housing Benefit — Applicable Amounts

**Primary authority**: Social Security Contributions and Benefits Act 1992 s.130;
Housing Benefit (General) Regulations 1987 (SI 1987/1971) Sch.2 as amended by annual
uprating orders.

Housing Benefit applicable amounts are set to mirror Income Support applicable amounts
(HB Regs 1987 Sch.2 para.1 is substituted by the same figures as IS Regs 1987 Sch.2
para.1). Accordingly the personal allowance rates in **§C.7** above also apply to HB.

The key HB-specific parameters are:

#### C.9.1 LHA and maximum rent restriction (from 2008/09)

Before 2008/09, HB for private tenants was restricted to the local reference rent. From
7 April 2008, HB for new claimants in private rented sector was determined by the Local
Housing Allowance (LHA) based on the 30th percentile (originally 50th) of local rents
in each Broad Rental Market Area (BRMA). Authority: Housing Benefit Regulations 2006
(SI 2006/213) reg.13D (inserted by SI 2007/2868 art.4).

From April 2011, LHA was reduced to the 30th percentile of local rents (SI 2010/2591).

#### C.9.2 Benefit Cap (from 2013/14)

Introduced by Welfare Reform Act 2012 s.96 and the Benefit Cap (Housing Benefit)
Regulations 2012 (SI 2012/2994).

| FY | London (family) | National (family) | London (single) | National (single) | Source |
|----|-----------------|-------------------|-----------------|--------------------|--------|
| 2013/14 | £500/wk | £500/wk | £500/wk | £500/wk | SI 2012/2994 reg.5 (one initial cap) |
| 2014/15–2015/16 | £500/wk | £500/wk | £500/wk | £500/wk | No change |
| 2016/17 | £442.31/wk (£23,000/yr) | £384.62/wk (£20,000/yr) | £296.35/wk (£15,410/yr) | £257.69/wk (£13,400/yr) | SI 2016/909 reg.2 (Welfare Reform and Work Act 2016 s.8) |
| 2017/18–2022/23 | (frozen) | (frozen) | (frozen) | (frozen) | SI 2012/2994 as amended by SI 2016/909 |

---

### C.10 NI Contribution Rates — Summary of Class 1 Rate Changes

| FY | Employee main | Employee additional (UEL+) | Employer | Source (primary) |
|----|--------------|---------------------------|----------|-----------------|
| 1994/95–2002/03 | 10% | 0% | 10.2%–12.2% | SSCBA 1992 s.8(2)(a); s.9(2) (as originally enacted); Finance Act 1994 |
| 2003/04+ | 11% (main) | 1% (additional) | 12.8% | National Insurance Contributions Act 2002 s.1 (SSCBA 1992 ss.8–9 substituted) |
| 2011/12+ | 12% (main) | 2% (additional) | 13.8% | National Insurance Contributions Act 2008 s.3 (effective from SI 2011/940) |

**Note**: Employer Class 1 secondary rate was reduced temporarily:
- 1999/00: 12.2% (set by SSCBA 1992 s.9 unamended)
- 2001/02: 11.9% (SI 2001/477 art.2, made under SSCBA 1992 s.9(4))
- 2002/03 onwards: reverted to 12.2% / then 12.8% from 2003/04

---

### C.11 Key Repeals and Replacements

The following legacy benefits are relevant to the historical period and to the model's
managed-migration logic:

| Benefit | Introduced | Replaced by | Authority |
|---------|-----------|------------|-----------|
| Family Credit | 1988 | Working Tax Credit (April 2003) | Tax Credits Act 2002 s.1(1) |
| Disabled Person's Tax Credit | 1999 | WTC disability element (2003) | Tax Credits Act 2002 s.1(1) |
| Working Families' Tax Credit | 1999 | Working Tax Credit (2003) | Tax Credits Act 2002 |
| Income Support (working-age) | 1988 | Universal Credit (roll-out 2013–) | Welfare Reform Act 2012 s.1 |
| Income-based JSA | 1996 | Universal Credit | Welfare Reform Act 2012 s.33 |
| Income-related ESA | 2008 | Universal Credit | Welfare Reform Act 2012 s.33 |
| Housing Benefit (working-age) | 1983 | UC housing element | Welfare Reform Act 2012 s.33 |
| Child Tax Credit | 2003 | UC child element | Welfare Reform Act 2012 |
| Working Tax Credit | 2003 | UC work allowances / standard allowance | Welfare Reform Act 2012 |

---

### C.12 Income Tax Rates 1994/95–2022/23

| FY | Starter/lower | Basic rate | Higher rate | Additional rate | Source |
|----|--------------|-----------|------------|----------------|--------|
| 1994/95–1995/96 | 20% (£0–2,500) | 25% | 40% | — | ICTA 1988 s.1(2) as amended FA 1994/1995 |
| 1996/97–1998/99 | 20% | 23% | 40% | — | FA 1996/1997/1998 (basic rate reduced) |
| 1999/00–2007/08 | 10% (£0–1,520→2,230) | 22% | 40% | — | FA 1999 (10p rate) → FA 2008 s.5 removed 10p |
| 2008/09 | — | 20% | 40% | — | FA 2008 s.5 (10p rate abolished; SSCBA 1992 s.1 et al) |
| 2009/10 | — | 20% | 40% | — | |
| 2010/11 | — | 20% | 40% | 50% (£150k+) | FA 2009 s.6 (additional rate introduced) |
| 2011/12–2012/13 | — | 20% | 40% | 50% | FA 2011 |
| 2013/14+ | — | 20% | 40% | 45% | FA 2013 s.2 (additional rate reduced from 50% to 45%) |

The 10% starting rate band was introduced by FA 1999 and applied to the first £1,520 of
taxable income (raised to £2,230 by 2007/08). It was abolished from 2008/09 by FA 2008
s.5 (amending ITA 2007 s.10; ICTA 1988 s.1 had already been repealed by ITA 2007).

---

*All SIs referenced above are available at legislation.gov.uk under their respective
`uksi/{year}/{number}` identifiers. FA = Finance Act (`ukpga/{year}/{number}`).
ICTA 1988 = Income and Corporation Taxes Act 1988 (`ukpga/1988/1`).
SSCBA 1992 = Social Security Contributions and Benefits Act 1992 (`ukpga/1992/4`).
SSAA 1992 = Social Security Administration Act 1992 (`ukpga/1992/5`).
TCA 2002 = Tax Credits Act 2002 (`ukpga/2002/21`).
ITA 2007 = Income Tax Act 2007 (`ukpga/2007/3`).
WRWA 2016 = Welfare Reform and Work Act 2016 (`ukpga/2016/7`).*
