use crate::engine::entities::{Household, Person, Region};
use crate::parameters::{CouncilTaxParams, CapitalGainsTaxParams, StampDutyParams, WealthTaxParams};

/// Determine the council tax band (0=A .. 7=H) from a 1991 property value.
///
/// The WAS `main_residence_value` is in current prices, not 1991 values, so this
/// is an approximation. For baseline runs we use the reported FRS council_tax; this
/// function is used for reform modelling (e.g. changing Band D rate).
pub fn council_tax_band(property_value: f64, thresholds: &[f64]) -> usize {
    for (i, &t) in thresholds.iter().enumerate().rev() {
        if property_value >= t {
            return i;
        }
    }
    0
}

/// Calculate council tax from parameters (for reform modelling).
///
/// Returns the Band D rate multiplied by the band multiplier for this household's
/// property value. For baseline runs, the simulation uses the reported `hh.council_tax`
/// instead.
///
/// Applies the single-person discount when `is_single_adult` is true (only one
/// adult aged 18+ is resident). Local Government Finance Act 1992 s.11(1)(a).
pub fn calculate_council_tax(
    hh: &Household,
    params: &CouncilTaxParams,
    is_single_adult: bool,
) -> f64 {
    let band = council_tax_band(hh.main_residence_value, &params.band_thresholds);
    let multiplier = params.band_multipliers.get(band).copied().unwrap_or(1.0);
    let gross = params.average_band_d * multiplier;
    if is_single_adult {
        gross * (1.0 - params.single_person_discount_rate)
    } else {
        gross
    }
}

/// Calculate capital gains tax for a person.
///
/// Uses the `capital_gains` field directly. Defaults to zero when no capital
/// gains data is available (FRS, WAS, SPI do not record realised gains).
/// The `is_higher_rate` flag should be true if the person's taxable income exceeds
/// the basic rate limit (i.e. they pay income tax at the higher/additional rate).
///
/// Residential property gains receive the configured `residential_surcharge`
/// on top of the basic / higher rate. The fraction of `capital_gains` that
/// counts as residential is taken from `Person.capital_gains_residential_share`
/// (default 0.0). The AEA is allocated pro-rata across the two slices, mirroring
/// the simplest case where the taxpayer cannot pick which gains the AEA covers.
pub fn calculate_capital_gains_tax(
    person: &Person,
    params: &CapitalGainsTaxParams,
    is_higher_rate: bool,
) -> f64 {
    let taxable_gains = (person.capital_gains - params.annual_exempt_amount).max(0.0);

    if taxable_gains <= 0.0 {
        return 0.0;
    }

    let rate = if is_higher_rate { params.higher_rate } else { params.basic_rate };
    let residential_share = person.capital_gains_residential_share.clamp(0.0, 1.0);
    let residential_taxable = taxable_gains * residential_share;
    let non_residential_taxable = taxable_gains - residential_taxable;

    non_residential_taxable * rate
        + residential_taxable * (rate + params.residential_surcharge)
}

/// Calculate stamp duty land tax on a property value using marginal bands.
///
/// SDLT is a slab/marginal tax: each band's rate applies only to the portion of the
/// price within that band (not to the entire price).
fn marginal_sdlt(property_value: f64, bands: &[crate::parameters::StampDutyBand]) -> f64 {
    if bands.is_empty() || property_value <= 0.0 {
        return 0.0;
    }

    let mut tax = 0.0;
    for i in 0..bands.len() {
        let lower = bands[i].threshold;
        let upper = if i + 1 < bands.len() { bands[i + 1].threshold } else { f64::MAX };
        let rate = bands[i].rate;

        if property_value <= lower {
            break;
        }

        let taxable = property_value.min(upper) - lower;
        tax += taxable.max(0.0) * rate;
    }

    tax
}

/// Calculate annualised stamp duty for a household.
///
/// Multiplies the one-off SDLT liability by the annual purchase probability
/// (1 / average holding period) to get an expected annual amount.
pub fn calculate_stamp_duty(hh: &Household, params: &StampDutyParams) -> f64 {
    let sdlt = marginal_sdlt(hh.main_residence_value, &params.bands);
    sdlt * params.annual_purchase_probability
}

/// Calculate annualised property-transaction tax for a household, dispatching
/// to the regime that applies in the household's region.
///
/// - Scotland → LBTT (Land and Buildings Transaction Tax (Scotland) Act 2013)
/// - Wales    → LTT  (Land Transaction Tax and Anti-avoidance of Devolved Taxes (Wales) Act 2017)
/// - elsewhere (England + NI) → SDLT (Finance Act 2003 s.55)
///
/// Each parameter argument is optional; the function returns 0.0 when the
/// regime that would apply is unset (e.g. no LBTT params loaded for a Scottish
/// household), matching the existing behaviour for missing SDLT params.
pub fn calculate_property_transaction_tax(
    hh: &Household,
    sdlt: Option<&StampDutyParams>,
    lbtt: Option<&StampDutyParams>,
    ltt:  Option<&StampDutyParams>,
) -> f64 {
    let params = match hh.region {
        Region::Scotland => lbtt,
        Region::Wales    => ltt,
        _                => sdlt,
    };
    params.map(|p| calculate_stamp_duty(hh, p)).unwrap_or(0.0)
}

/// Calculate annual wealth tax for a household.
///
/// Hypothetical flat-rate tax on net wealth above a threshold.
pub fn calculate_wealth_tax(hh: &Household, params: &WealthTaxParams) -> f64 {
    if !params.enabled {
        return 0.0;
    }

    let total_wealth = hh.property_wealth + hh.corporate_wealth + hh.gross_financial_wealth;
    let taxable = (total_wealth - params.threshold).max(0.0);
    taxable * params.rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::{Household, Person};
    use crate::parameters::{
        CouncilTaxParams, CapitalGainsTaxParams, StampDutyParams, StampDutyBand, WealthTaxParams,
    };

    #[test]
    fn council_tax_band_lookup() {
        let thresholds = vec![0.0, 40001.0, 52001.0, 68001.0, 88001.0, 120001.0, 160001.0, 320001.0];
        assert_eq!(council_tax_band(30000.0, &thresholds), 0); // Band A
        assert_eq!(council_tax_band(50000.0, &thresholds), 1); // Band B
        assert_eq!(council_tax_band(100000.0, &thresholds), 4); // Band E
        assert_eq!(council_tax_band(500000.0, &thresholds), 7); // Band H
    }

    fn make_council_tax_params() -> CouncilTaxParams {
        CouncilTaxParams {
            average_band_d: 2280.0,
            band_multipliers: vec![6.0/9.0, 7.0/9.0, 8.0/9.0, 1.0, 11.0/9.0, 13.0/9.0, 15.0/9.0, 18.0/9.0],
            band_thresholds: vec![0.0, 40001.0, 52001.0, 68001.0, 88001.0, 120001.0, 160001.0, 320001.0],
            single_person_discount_rate: 0.25,
        }
    }

    #[test]
    fn council_tax_calculation() {
        let params = make_council_tax_params();
        let mut hh = Household::default();
        hh.main_residence_value = 80000.0; // Band D
        let ct = calculate_council_tax(&hh, &params, false);
        assert!((ct - 2280.0).abs() < 1.0); // Band D = 1.0 * band_d
    }

    #[test]
    fn council_tax_single_person_discount() {
        let params = make_council_tax_params();
        let mut hh = Household::default();
        hh.main_residence_value = 80000.0; // Band D
        let ct_full   = calculate_council_tax(&hh, &params, false);
        let ct_single = calculate_council_tax(&hh, &params, true);
        assert!((ct_full - 2280.0).abs() < 1.0);
        // 25% discount: £2,280 × 0.75 = £1,710
        assert!((ct_single - 1710.0).abs() < 1.0, "got {}", ct_single);
    }

    #[test]
    fn council_tax_single_person_discount_band_a() {
        let params = make_council_tax_params();
        let mut hh = Household::default();
        hh.main_residence_value = 30000.0; // Band A
        let band_a_full = 2280.0 * 6.0 / 9.0; // = £1,520
        let ct_full   = calculate_council_tax(&hh, &params, false);
        let ct_single = calculate_council_tax(&hh, &params, true);
        assert!((ct_full - band_a_full).abs() < 1.0);
        assert!((ct_single - band_a_full * 0.75).abs() < 1.0);
    }

    #[test]
    fn council_tax_zero_discount_rate_no_discount() {
        let mut params = make_council_tax_params();
        params.single_person_discount_rate = 0.0;
        let mut hh = Household::default();
        hh.main_residence_value = 80000.0;
        let ct_full   = calculate_council_tax(&hh, &params, false);
        let ct_single = calculate_council_tax(&hh, &params, true);
        assert_eq!(ct_full, ct_single);
    }

    #[test]
    fn cgt_basic_rate() {
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.18,
            higher_rate: 0.24,
            residential_surcharge: 0.0,
        };
        let mut p = Person::default();
        p.capital_gains = 8000.0;
        // taxable = 8000 - 3000 = 5000; cgt = 5000 * 0.18 = 900
        let cgt = calculate_capital_gains_tax(&p, &params, false);
        assert!((cgt - 900.0).abs() < 0.01);
    }

    #[test]
    fn cgt_higher_rate() {
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.18,
            higher_rate: 0.24,
            residential_surcharge: 0.0,
        };
        let mut p = Person::default();
        p.capital_gains = 8000.0;
        // taxable = 5000; cgt = 5000 * 0.24 = 1200
        let cgt = calculate_capital_gains_tax(&p, &params, true);
        assert!((cgt - 1200.0).abs() < 0.01);
    }

    #[test]
    fn cgt_below_exempt() {
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.18,
            higher_rate: 0.24,
            residential_surcharge: 0.0,
        };
        let mut p = Person::default();
        p.capital_gains = 1000.0; // below AEA
        assert_eq!(calculate_capital_gains_tax(&p, &params, false), 0.0);
    }

    #[test]
    fn cgt_zero_by_default() {
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.18,
            higher_rate: 0.24,
            residential_surcharge: 0.0,
        };
        // No capital_gains set — should produce zero (FRS/WAS default behaviour)
        let p = Person::default();
        assert_eq!(calculate_capital_gains_tax(&p, &params, false), 0.0);
    }

    #[test]
    fn cgt_residential_surcharge_full_residential() {
        // 2023/24-style residential surcharge: higher rate 20%, surcharge 8 pp -> 28% on residential.
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.10,
            higher_rate: 0.20,
            residential_surcharge: 0.08,
        };
        let mut p = Person::default();
        p.capital_gains = 8000.0;
        p.capital_gains_residential_share = 1.0;
        // taxable = 5000; rate = 20% + 8% = 28%; cgt = 1400
        let cgt = calculate_capital_gains_tax(&p, &params, true);
        assert!((cgt - 1400.0).abs() < 0.01);
    }

    #[test]
    fn cgt_residential_surcharge_mixed() {
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.10,
            higher_rate: 0.20,
            residential_surcharge: 0.08,
        };
        let mut p = Person::default();
        p.capital_gains = 8000.0;
        p.capital_gains_residential_share = 0.4; // 40% residential
        // taxable = 5000; residential portion = 2000 at 28% = 560;
        // non-residential portion = 3000 at 20% = 600; total = 1160.
        let cgt = calculate_capital_gains_tax(&p, &params, true);
        assert!((cgt - 1160.0).abs() < 0.01);
    }

    #[test]
    fn cgt_residential_share_clamped() {
        // Out-of-range shares are clamped to [0, 1].
        let params = CapitalGainsTaxParams {
            annual_exempt_amount: 3000.0,
            basic_rate: 0.10,
            higher_rate: 0.20,
            residential_surcharge: 0.08,
        };
        let mut p = Person::default();
        p.capital_gains = 8000.0;
        p.capital_gains_residential_share = 5.0; // nonsense, clamps to 1.0
        let cgt = calculate_capital_gains_tax(&p, &params, true);
        // taxable = 5000; full residential at 28% = 1400
        assert!((cgt - 1400.0).abs() < 0.01);
    }

    #[test]
    fn stamp_duty_marginal() {
        let params = StampDutyParams {
            bands: vec![
                StampDutyBand { rate: 0.0, threshold: 0.0 },
                StampDutyBand { rate: 0.02, threshold: 125001.0 },
                StampDutyBand { rate: 0.05, threshold: 250001.0 },
                StampDutyBand { rate: 0.10, threshold: 925001.0 },
                StampDutyBand { rate: 0.12, threshold: 1500001.0 },
            ],
            annual_purchase_probability: 1.0, // set to 1 for testing one-off amount
        };
        let mut hh = Household::default();
        hh.main_residence_value = 500000.0;
        // 0% on first £125k, 2% on £125k-£250k = £2,500, 5% on £250k-£500k = £12,500
        // total = £15,000
        let sdlt = calculate_stamp_duty(&hh, &params);
        assert!((sdlt - 15000.0).abs() < 1.0);
    }

    fn make_sdlt() -> StampDutyParams {
        StampDutyParams {
            bands: vec![
                StampDutyBand { rate: 0.0,  threshold: 0.0 },
                StampDutyBand { rate: 0.02, threshold: 125001.0 },
                StampDutyBand { rate: 0.05, threshold: 250001.0 },
                StampDutyBand { rate: 0.10, threshold: 925001.0 },
                StampDutyBand { rate: 0.12, threshold: 1500001.0 },
            ],
            annual_purchase_probability: 1.0,
        }
    }

    fn make_lbtt() -> StampDutyParams {
        // Scotland 2025/26 (residential).
        StampDutyParams {
            bands: vec![
                StampDutyBand { rate: 0.0,  threshold: 0.0 },
                StampDutyBand { rate: 0.02, threshold: 145001.0 },
                StampDutyBand { rate: 0.05, threshold: 250001.0 },
                StampDutyBand { rate: 0.10, threshold: 325001.0 },
                StampDutyBand { rate: 0.12, threshold: 750001.0 },
            ],
            annual_purchase_probability: 1.0,
        }
    }

    fn make_ltt() -> StampDutyParams {
        // Wales 2025/26 (residential primary).
        StampDutyParams {
            bands: vec![
                StampDutyBand { rate: 0.0,    threshold: 0.0 },
                StampDutyBand { rate: 0.035,  threshold: 180001.0 },
                StampDutyBand { rate: 0.05,   threshold: 250001.0 },
                StampDutyBand { rate: 0.075,  threshold: 400001.0 },
                StampDutyBand { rate: 0.10,   threshold: 750001.0 },
                StampDutyBand { rate: 0.12,   threshold: 1500001.0 },
            ],
            annual_purchase_probability: 1.0,
        }
    }

    #[test]
    fn property_tax_routes_to_lbtt_in_scotland() {
        let mut hh = Household::default();
        hh.main_residence_value = 500_000.0;
        hh.region = Region::Scotland;
        // LBTT on £500k:
        //   0% on first £145k = £0
        //   2% on £145k-£250k (£105k) = £2,100
        //   5% on £250k-£325k (£75k)  = £3,750
        //  10% on £325k-£500k (£175k) = £17,500
        //  total                      = £23,350
        let tax = calculate_property_transaction_tax(
            &hh, Some(&make_sdlt()), Some(&make_lbtt()), Some(&make_ltt())
        );
        assert!((tax - 23_350.0).abs() < 1.0, "got {}", tax);
    }

    #[test]
    fn property_tax_routes_to_ltt_in_wales() {
        let mut hh = Household::default();
        hh.main_residence_value = 500_000.0;
        hh.region = Region::Wales;
        // LTT on £500k:
        //   0%   on first £180k                = £0
        //   3.5% on £180k-£250k (£70k)         = £2,450
        //   5%   on £250k-£400k (£150k)        = £7,500
        //   7.5% on £400k-£500k (£100k)        = £7,500
        //   total                               = £17,450
        let tax = calculate_property_transaction_tax(
            &hh, Some(&make_sdlt()), Some(&make_lbtt()), Some(&make_ltt())
        );
        assert!((tax - 17_450.0).abs() < 1.0, "got {}", tax);
    }

    #[test]
    fn property_tax_routes_to_sdlt_outside_scotland_and_wales() {
        let mut hh = Household::default();
        hh.main_residence_value = 500_000.0;
        hh.region = Region::London;
        // Same as the existing stamp_duty_marginal test: £15,000.
        let tax = calculate_property_transaction_tax(
            &hh, Some(&make_sdlt()), Some(&make_lbtt()), Some(&make_ltt())
        );
        assert!((tax - 15_000.0).abs() < 1.0, "got {}", tax);
    }

    #[test]
    fn property_tax_returns_zero_when_devolved_params_missing() {
        let mut hh = Household::default();
        hh.main_residence_value = 500_000.0;
        hh.region = Region::Scotland;
        // No LBTT params loaded → tax is 0 (regime doesn't fall back to SDLT).
        let tax = calculate_property_transaction_tax(&hh, Some(&make_sdlt()), None, None);
        assert_eq!(tax, 0.0);
    }

    #[test]
    fn lbtt_zero_below_nil_band() {
        let mut hh = Household::default();
        hh.main_residence_value = 100_000.0; // below £145k LBTT nil-band ceiling
        hh.region = Region::Scotland;
        let tax = calculate_property_transaction_tax(
            &hh, Some(&make_sdlt()), Some(&make_lbtt()), Some(&make_ltt())
        );
        assert_eq!(tax, 0.0);
    }

    #[test]
    fn ltt_zero_below_nil_band() {
        let mut hh = Household::default();
        hh.main_residence_value = 150_000.0; // below £180k LTT nil-band ceiling
        hh.region = Region::Wales;
        let tax = calculate_property_transaction_tax(
            &hh, Some(&make_sdlt()), Some(&make_lbtt()), Some(&make_ltt())
        );
        assert_eq!(tax, 0.0);
    }

    #[test]
    fn wealth_tax_disabled() {
        let params = WealthTaxParams { enabled: false, threshold: 10_000_000.0, rate: 0.01 };
        let mut hh = Household::default();
        hh.property_wealth = 50_000_000.0;
        assert_eq!(calculate_wealth_tax(&hh, &params), 0.0);
    }

    #[test]
    fn wealth_tax_above_threshold() {
        let params = WealthTaxParams { enabled: true, threshold: 10_000_000.0, rate: 0.01 };
        let mut hh = Household::default();
        hh.property_wealth = 12_000_000.0;
        hh.corporate_wealth = 3_000_000.0;
        hh.gross_financial_wealth = 0.0;
        // total = 15m; taxable = 5m; tax = 50,000
        let tax = calculate_wealth_tax(&hh, &params);
        assert!((tax - 50_000.0).abs() < 0.01);
    }
}
