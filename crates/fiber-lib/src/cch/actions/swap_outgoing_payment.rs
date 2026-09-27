//! Swap (Fiber-to-Fiber) outgoing-payment dispatch.
//!
//! Mirrors [`SendOutgoingPaymentDispatcher`](super::send_outgoing_payment::SendOutgoingPaymentDispatcher)
//! for [`SwapOrder`]s, reusing the existing
//! [`SendFiberOutgoingPaymentExecutor`](super::send_outgoing_payment::SendFiberOutgoingPaymentExecutor)
//! unchanged. Only the inputs differ: pair/rate/fees come from the consumed
//! [`SwapAdvertisement`](fiber_types::SwapAdvertisement) via the order,
//! amounts are UDT units (not sats), and both legs are Fiber-native.
//!
//! Actor wiring (sending the built executor, failure events) belongs to S3;
//! this module is pure dispatch logic plus unit tests.

use super::send_outgoing_payment::{outgoing_max_fee_rate, SendFiberOutgoingPaymentExecutor};
use crate::cch::cch_fiber_agent::OutgoingFeeLimit;
use crate::cch::{CchFiberAgentRef, CchMessage};
use crate::fiber::config::MAX_PAYMENT_TLC_EXPIRY_LIMIT;
use crate::now_timestamp_as_millis_u64;
use fiber_types::{CchOrderStatus, SwapOrder};
use ractor::ActorRef;

pub struct SwapOutgoingPaymentDispatcher;

impl SwapOutgoingPaymentDispatcher {
    pub fn should_dispatch(order: &SwapOrder) -> bool {
        order.status == CchOrderStatus::IncomingAccepted
    }

    /// Fee budget in quote units: the order's own fee allowance, so routing
    /// can never consume more than the ad's fee terms permit.
    /// `fee_rate` is per-million of `quote_amount` (mirrors the CCH invariant
    /// that outgoing route fees stay within the collected fee).
    pub fn fee_budget_quote_units(order: &SwapOrder) -> u128 {
        order
            .quote_amount
            .saturating_mul(order.fee_rate)
            .checked_div(1_000_000)
            .unwrap_or(0)
    }

    /// Maximum outgoing route time in seconds: half the remaining life of
    /// the incoming (base) leg, reserving the other half for settling it
    /// after preimage reveal. Mirrors the CCH half-time rule.
    pub fn max_outgoing_expiry_seconds(
        order: &SwapOrder,
        now_secs: u64,
        fallback_delta_seconds: u64,
    ) -> Option<u64> {
        let elapsed = now_secs.saturating_sub(order.created_at);
        // CkbInvoice stores the delta in ms; convert to seconds.
        let incoming_expiry_seconds = order
            .base_invoice
            .final_tlc_minimum_expiry_delta()
            .map(|ms| ms / 1000)
            .unwrap_or(fallback_delta_seconds);
        let remaining = incoming_expiry_seconds.checked_sub(elapsed)?;
        let half = remaining / 2;
        if half == 0 {
            return None;
        }
        Some(half)
    }

    /// Build the (existing) Fiber payment executor for the quote leg.
    #[allow(clippy::too_many_arguments)]
    pub fn build_executor(
        fiber_agent_ref: CchFiberAgentRef,
        cch_actor_ref: ActorRef<CchMessage>,
        order: &SwapOrder,
        tlc_expiry_limit_ms: u64,
        retry_count: u32,
    ) -> SendFiberOutgoingPaymentExecutor {
        let fee_budget = Self::fee_budget_quote_units(order);
        SendFiberOutgoingPaymentExecutor::new(
            order.payment_hash,
            cch_actor_ref,
            fiber_agent_ref,
            order.quote_pay_req.clone(),
            retry_count,
            tlc_expiry_limit_ms.min(MAX_PAYMENT_TLC_EXPIRY_LIMIT),
            OutgoingFeeLimit {
                max_fee_amount: fee_budget,
                max_fee_rate: outgoing_max_fee_rate(order.quote_amount, fee_budget),
            },
        )
    }

    /// Current time in seconds (seams for tests).
    pub fn now_secs() -> u64 {
        now_timestamp_as_millis_u64() / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::InvoiceBuilder;
    use ckb_types::packed::Script;
    use ckb_types::prelude::{Builder, Entity};
    use fiber_types::invoice::Currency;
    use fiber_types::{CchOrderStatus, Hash256};

    fn dummy_script(byte: u8) -> Script {
        let mut h = [0u8; 32];
        h[0] = byte;
        Script::new_builder()
            .code_hash(ckb_types::packed::Byte32::from_slice(&h).unwrap())
            .hash_type(ckb_types::core::ScriptHashType::Type)
            .args(ckb_types::packed::Bytes::from(vec![byte]))
            .build()
    }

    fn dummy_invoice() -> fiber_types::CkbInvoice {
        use secp256k1::SECP256K1;
        let secret = secp256k1::SecretKey::from_slice(&[42u8; 32]).unwrap();
        InvoiceBuilder::new(Currency::Fibd)
            .amount(Some(100))
            .payment_hash(Hash256::default())
            .build_with_sign(|msg| SECP256K1.sign_ecdsa_recoverable(msg, &secret))
            .unwrap()
    }

    fn make_order() -> SwapOrder {
        SwapOrder::new(
            1_700_000_000,
            3_600,
            Hash256::default(),
            Some(dummy_script(1)),
            Some(dummy_script(2)),
            10_000_000_000,
            3_000,
            100,
            Hash256::default(),
            dummy_invoice(),
            "fiber-pay-req".to_string(),
        )
        .unwrap()
    }

    #[test]
    fn swap_dispatch_only_when_incoming_accepted() {
        let mut order = make_order();
        assert!(!SwapOutgoingPaymentDispatcher::should_dispatch(&order));
        order.status = CchOrderStatus::IncomingAccepted;
        assert!(SwapOutgoingPaymentDispatcher::should_dispatch(&order));
        order.status = CchOrderStatus::Success;
        assert!(!SwapOutgoingPaymentDispatcher::should_dispatch(&order));
    }

    #[test]
    fn swap_fee_budget_stays_within_ad_terms() {
        // quote 10_000 units (100 base @ 100.0) @ 3000/million -> 30 units.
        let order = make_order();
        assert_eq!(
            SwapOutgoingPaymentDispatcher::fee_budget_quote_units(&order),
            30
        );
    }

    #[test]
    fn swap_expiry_uses_half_time_rule() {
        let order = make_order();
        // Invoice built without explicit final-expiry attr: falls back to
        // the passed default (use 7200s here).
        // created_at=1_700_000_000, now=+100s -> elapsed 100.
        let max = SwapOutgoingPaymentDispatcher::max_outgoing_expiry_seconds(
            &order,
            1_700_000_100,
            7_200,
        )
        .unwrap();
        assert_eq!(max, (7_200 - 100) / 2);
        // Exhausted time -> None (caller fails the order in S3).
        assert!(
            SwapOutgoingPaymentDispatcher::max_outgoing_expiry_seconds(
                &order,
                1_700_000_000 + 7_200,
                7_200,
            )
            .is_none()
        );
    }
}
