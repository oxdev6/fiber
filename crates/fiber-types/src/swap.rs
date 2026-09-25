//! Fiber-to-Fiber swap orders (RGB++ market execution).
//!
//! Unlike [`CchOrder`](crate::cch::CchOrder), which bridges Lightning and
//! Fiber, a [`SwapOrder`] has **both legs on Fiber/CKB**: the user pays the
//! hub's base-asset invoice (leg A) and the hub pays the user's quote-asset
//! invoice (leg B), linked by one payment hash. The traded pair and rate
//! come from a consumed [`SwapAdvertisement`](crate::SwapAdvertisement).

use crate::cch::CchOrderStatus;
use crate::invoice::CkbInvoice;
use crate::serde_utils::{EntityHex, U128Hex, U64Hex};
use crate::Hash256;
use ckb_types::packed::Script;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use thiserror::Error;

/// Errors constructing or transitioning a [`SwapOrder`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SwapOrderError {
    #[error("rate must be non-zero")]
    ZeroRate,
    #[error("base amount must be non-zero")]
    ZeroAmount,
    #[error("quote amount overflow: base {0} * rate {1}")]
    QuoteOverflow(u128, u128),
    #[error("expiry must be after creation")]
    BadExpiry,
}

/// A Fiber-to-Fiber atomic swap order.
///
/// Amounts are in UDT base units; `rate` is quote-per-base × 1e8, frozen at
/// creation so later routing fees can never silently move the price.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapOrder {
    /// Seconds since epoch when the order is created.
    #[serde_as(as = "U64Hex")]
    pub created_at: u64,
    /// Relative expiry in seconds from `created_at`.
    #[serde_as(as = "U64Hex")]
    pub expiry_delta_seconds: u64,

    /// blake2b of the consumed SwapAdvertisement (audit trail, not a FK).
    pub ad_hash: Hash256,
    /// Exact scripts from the advertisement (base = leaped UDT).
    #[serde_as(as = "Option<EntityHex>")]
    pub base_script: Option<Script>,
    #[serde_as(as = "Option<EntityHex>")]
    pub quote_script: Option<Script>,

    /// Frozen rate (quote per base × 1e8) and fee (per million).
    #[serde_as(as = "U128Hex")]
    pub rate: u128,
    #[serde_as(as = "U128Hex")]
    pub fee_rate: u128,

    /// User-specified base units in, derived quote units out.
    #[serde_as(as = "U128Hex")]
    pub base_amount: u128,
    #[serde_as(as = "U128Hex")]
    pub quote_amount: u128,

    /// Links both legs; also the store key.
    pub payment_hash: Hash256,
    pub payment_preimage: Option<Hash256>,

    /// Hub's base-asset invoice the user pays (leg A).
    pub base_invoice: CkbInvoice,
    /// User's quote-asset invoice the hub pays (leg B), serialized.
    pub quote_pay_req: String,

    pub status: CchOrderStatus,
    pub failure_reason: Option<String>,
}

impl SwapOrder {
    /// Build an order, freezing rate math with checked arithmetic.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        created_at: u64,
        expiry_delta_seconds: u64,
        ad_hash: Hash256,
        base_script: Option<Script>,
        quote_script: Option<Script>,
        rate: u128,
        fee_rate: u128,
        base_amount: u128,
        payment_hash: Hash256,
        base_invoice: CkbInvoice,
        quote_pay_req: String,
    ) -> Result<Self, SwapOrderError> {
        if rate == 0 {
            return Err(SwapOrderError::ZeroRate);
        }
        if base_amount == 0 {
            return Err(SwapOrderError::ZeroAmount);
        }
        if expiry_delta_seconds == 0 {
            return Err(SwapOrderError::BadExpiry);
        }
        let quote_amount = base_amount
            .checked_mul(rate)
            .and_then(|v| v.checked_div(100_000_000))
            .ok_or(SwapOrderError::QuoteOverflow(base_amount, rate))?;
        Ok(Self {
            created_at,
            expiry_delta_seconds,
            ad_hash,
            base_script,
            quote_script,
            rate,
            fee_rate,
            base_amount,
            quote_amount,
            payment_hash,
            payment_preimage: None,
            base_invoice,
            quote_pay_req,
            status: CchOrderStatus::Pending,
            failure_reason: None,
        })
    }

    pub fn is_final(&self) -> bool {
        self.status == CchOrderStatus::Success || self.status == CchOrderStatus::Failed
    }

    /// Mark Failed if expired; returns true when it expired here.
    pub fn update_if_expired(&mut self, current_time: u64) -> bool {
        if !self.is_final()
            && current_time >= self.created_at.saturating_add(self.expiry_delta_seconds)
        {
            self.status = CchOrderStatus::Failed;
            self.failure_reason = Some("Swap order expired".to_string());
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_script(byte: u8) -> Script {
        use ckb_types::prelude::{Builder, Entity};
        let mut h = [0u8; 32];
        h[0] = byte;
        Script::new_builder()
            .code_hash(ckb_types::packed::Byte32::from_slice(&h).unwrap())
            .hash_type(ckb_types::core::ScriptHashType::Type)
            .args(ckb_types::packed::Bytes::from(vec![byte]))
            .build()
    }

    fn dummy_invoice() -> CkbInvoice {
        // Content irrelevant for math/expiry tests; minimal literal.
        use crate::invoice::{Currency, InvoiceData};
        CkbInvoice {
            currency: Currency::Fibd,
            amount: Some(100),
            signature: None,
            data: InvoiceData {
                timestamp: 1,
                payment_hash: Hash256::default(),
                attrs: vec![],
            },
        }
    }

    fn make_order(rate: u128, base_amount: u128) -> Result<SwapOrder, SwapOrderError> {
        SwapOrder::new(
            1_000,
            3_600,
            Hash256::default(),
            Some(dummy_script(1)),
            Some(dummy_script(2)),
            rate,
            3_000,
            base_amount,
            Hash256::default(),
            dummy_invoice(),
            "fiber-invoice-placeholder".to_string(),
        )
    }

    #[test]
    fn swap_order_freezes_quote_math() {
        // 100 base units @ rate 200*1e8 -> 20000 quote units.
        let o = make_order(200_0000_0000, 100).unwrap();
        assert_eq!(o.quote_amount, 20_000);
        assert_eq!(o.status, CchOrderStatus::Pending);
        assert!(!o.is_final());
    }

    #[test]
    fn swap_order_rejects_zero_rate_and_amount() {
        assert!(matches!(make_order(0, 100), Err(SwapOrderError::ZeroRate)));
        assert!(matches!(
            make_order(100, 0),
            Err(SwapOrderError::ZeroAmount)
        ));
        assert!(make_order(100, 100).is_ok());
    }

    #[test]
    fn swap_order_rejects_overflow() {
        assert!(matches!(
            make_order(u128::MAX, u128::MAX),
            Err(SwapOrderError::QuoteOverflow(_, _))
        ));
    }

    #[test]
    fn swap_order_expiry_marks_failed() {
        let mut o = make_order(100, 100).unwrap();
        assert!(!o.update_if_expired(1_000 + 3_599));
        assert!(!o.is_final());
        assert!(o.update_if_expired(1_000 + 3_600));
        assert_eq!(o.status, CchOrderStatus::Failed);
        assert!(o.is_final());
        assert!(o.failure_reason.is_some());
        // Final orders stay final.
        assert!(!o.update_if_expired(u64::MAX));
    }

    #[test]
    fn swap_order_serde_roundtrip() {
        let o = make_order(10_000_000_000, 500).unwrap();
        let s = serde_json::to_string(&o).unwrap();
        let o2: SwapOrder = serde_json::from_str(&s).unwrap();
        assert_eq!(o.rate, o2.rate);
        assert_eq!(o.quote_amount, o2.quote_amount);
        assert_eq!(o.status, o2.status);
    }
}
