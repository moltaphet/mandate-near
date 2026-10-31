//! Exact USD valuation. Integer only, no floats, computed in 256 bits so no
//! intermediate product can overflow. Callers pick the rounding direction
//! that makes each check conservative.

use std::fmt;

use near_sdk::json_types::U64;
use near_sdk::near;

use crate::mandate::MAX_DECIMALS;

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
mod u256 {
    uint::construct_uint! {
        /// 256-bit unsigned integer for intermediate products.
        pub struct U256(4);
    }
}
pub use u256::U256;

/// Micro-dollars per dollar.
pub const USD_MICROS: u64 = 1_000_000;
/// Most negative price exponent accepted (Pyth-style feeds use about -8).
pub const MIN_PRICE_EXPO: i32 = -30;

/// USD price of one *whole* token: `mantissa × 10^expo`.
#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub mantissa: U64,
    /// Must be in `MIN_PRICE_EXPO..=0`.
    pub expo: i32,
    /// Unix seconds when the oracle observed the price.
    pub publish_time_sec: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    Down,
    Up,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    ZeroPrice,
    UnsupportedExponent(i32),
    UnsupportedDecimals(u8),
    /// The value does not fit in `u128` micro-dollars.
    Overflow,
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPrice => write!(f, "price is zero"),
            Self::UnsupportedExponent(e) => {
                write!(f, "price exponent {e} outside {MIN_PRICE_EXPO}..=0")
            }
            Self::UnsupportedDecimals(d) => write!(f, "decimals {d} above {MAX_DECIMALS}"),
            Self::Overflow => write!(f, "USD value overflows u128"),
        }
    }
}

/// USD value, in micro-dollars, of `amount` base units of a token with
/// `decimals` decimals at `price`:
///
/// `amount × mantissa × 10^6 / (10^decimals × 10^-expo)`, rounded as asked.
///
/// Bounds: amount < 2^128, mantissa < 2^64, 10^6 < 2^20, so the numerator
/// is below 2^212. The denominator is at most 10^60 < 2^200. Neither
/// overflows 256 bits.
pub fn usd_micros(
    amount: u128,
    decimals: u8,
    price: &Price,
    rounding: Rounding,
) -> Result<u128, MathError> {
    if price.mantissa.0 == 0 {
        return Err(MathError::ZeroPrice);
    }
    if !(MIN_PRICE_EXPO..=0).contains(&price.expo) {
        return Err(MathError::UnsupportedExponent(price.expo));
    }
    if decimals > MAX_DECIMALS {
        return Err(MathError::UnsupportedDecimals(decimals));
    }
    let numerator = U256::from(amount) * U256::from(price.mantissa.0) * U256::from(USD_MICROS);
    let denominator =
        U256::exp10(usize::from(decimals)) * U256::exp10(price.expo.unsigned_abs() as usize);
    let (quotient, remainder) = numerator.div_mod(denominator);
    let rounded = match rounding {
        Rounding::Up if !remainder.is_zero() => quotient + U256::one(),
        _ => quotient,
    };
    to_u128(rounded)
}

/// `a × b` in 256 bits, for cross-multiplied comparisons such as
/// `value_out × 10_000 >= value_in × (10_000 − slippage_bps)`.
pub fn wide_mul(a: u128, b: u128) -> U256 {
    U256::from(a) * U256::from(b)
}

fn to_u128(v: U256) -> Result<u128, MathError> {
    if v.bits() > 128 { Err(MathError::Overflow) } else { Ok(v.as_u128()) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    pub(crate) fn price(mantissa: u64, expo: i32, publish_time_sec: u64) -> Price {
        Price { mantissa: U64(mantissa), expo, publish_time_sec }
    }

    #[test]
    fn one_near_at_five_dollars() {
        // 1 wNEAR = 10^24 base units; price 5.25 USD = 525 × 10^-2.
        let p = price(525, -2, 0);
        assert_eq!(usd_micros(10u128.pow(24), 24, &p, Rounding::Down), Ok(5_250_000));
        assert_eq!(usd_micros(10u128.pow(24), 24, &p, Rounding::Up), Ok(5_250_000));
    }

    #[test]
    fn usdc_at_one_dollar_pyth_style() {
        // 2 USDC (6 decimals) at 0.99990000 USD (expo -8).
        let p = price(99_990_000, -8, 0);
        assert_eq!(usd_micros(2_000_000, 6, &p, Rounding::Down), Ok(1_999_800));
    }

    #[test]
    fn rounding_direction_on_fractions() {
        // 1 base unit of a 24-decimal token at 5 USD = 5e-18 USD = 5e-12 micro-USD.
        let p = price(5, 0, 0);
        assert_eq!(usd_micros(1, 24, &p, Rounding::Down), Ok(0));
        assert_eq!(usd_micros(1, 24, &p, Rounding::Up), Ok(1));
        assert_eq!(usd_micros(0, 24, &p, Rounding::Up), Ok(0));
    }

    #[test]
    fn rejects_unusable_prices() {
        assert_eq!(usd_micros(1, 6, &price(0, -8, 0), Rounding::Down), Err(MathError::ZeroPrice));
        assert_eq!(
            usd_micros(1, 6, &price(1, 1, 0), Rounding::Down),
            Err(MathError::UnsupportedExponent(1))
        );
        assert_eq!(
            usd_micros(1, 6, &price(1, MIN_PRICE_EXPO - 1, 0), Rounding::Down),
            Err(MathError::UnsupportedExponent(MIN_PRICE_EXPO - 1))
        );
        assert_eq!(
            usd_micros(1, MAX_DECIMALS + 1, &price(1, 0, 0), Rounding::Down),
            Err(MathError::UnsupportedDecimals(MAX_DECIMALS + 1))
        );
    }

    #[test]
    fn extreme_inputs_overflow_cleanly() {
        let p = price(u64::MAX, 0, 0);
        assert_eq!(usd_micros(u128::MAX, 0, &p, Rounding::Up), Err(MathError::Overflow));
    }

    proptest! {
        /// Down ≤ exact ≤ Up, and they differ by at most one micro-dollar.
        #[test]
        fn rounding_brackets_the_exact_value(
            amount in any::<u128>(),
            mantissa in 1u64..,
            expo in MIN_PRICE_EXPO..=0,
            decimals in 0u8..=MAX_DECIMALS,
        ) {
            let p = price(mantissa, expo, 0);
            let down = usd_micros(amount, decimals, &p, Rounding::Down);
            let up = usd_micros(amount, decimals, &p, Rounding::Up);
            match (down, up) {
                (Ok(d), Ok(u)) => {
                    prop_assert!(u - d <= 1);
                    let num = U256::from(amount) * U256::from(mantissa) * U256::from(USD_MICROS);
                    let den = U256::exp10(decimals as usize) * U256::exp10(expo.unsigned_abs() as usize);
                    prop_assert!(U256::from(d) * den <= num);
                    prop_assert!(U256::from(u) * den >= num);
                }
                // Overflow may only happen at the very top of the range, and
                // then Up must overflow too (it is never smaller than Down).
                (Err(MathError::Overflow), Err(MathError::Overflow)) | (Ok(_), Err(MathError::Overflow)) => {}
                other => prop_assert!(false, "unexpected {other:?}"),
            }
        }
    }
}
