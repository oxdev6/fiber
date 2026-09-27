//! Swap (Fiber-to-Fiber) action routing.
//!
//! Mirrors [`ActionDispatcher`](super::ActionDispatcher) for [`SwapOrder`]:
//! status → actions, plus async execution building the reused Fiber
//! executors. Tracking needs no per-order action — like CCH Fiber legs,
//! the actor already tracks all Fiber invoices globally, so incoming leg
//! events arrive via the shared [`CchTrackingEvent`] stream (wired in S3b-2).

use super::settle_incoming_invoice::SettleFiberIncomingInvoiceExecutor;
use super::swap_outgoing_payment::SwapOutgoingPaymentDispatcher;
use crate::cch::{CchFiberAgentRef, CchMessage};
use crate::fiber::config::MAX_PAYMENT_TLC_EXPIRY_LIMIT;
use anyhow::Result;
use fiber_types::{CchOrderStatus, SwapOrder};
use ractor::ActorRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapOrderAction {
    SendQuotePayment,
    SettleBaseInvoice,
}

/// Actions for a swap order entering a new status. Mirrors
/// `ActionDispatcher::on_entering` for CCH orders.
pub fn swap_on_entering(order: &SwapOrder) -> Vec<SwapOrderAction> {
    match order.status {
        CchOrderStatus::Pending => vec![],
        CchOrderStatus::IncomingAccepted => vec![SwapOrderAction::SendQuotePayment],
        CchOrderStatus::OutgoingInFlight => vec![],
        CchOrderStatus::OutgoingSuccess => vec![SwapOrderAction::SettleBaseInvoice],
        CchOrderStatus::Success => vec![],
        CchOrderStatus::Failed => vec![],
    }
}

/// Execute one swap action. Returns Ok on success or handled outcomes;
/// Err triggers scheduler retry (mirrors CCH `ExecuteAction` semantics).
pub async fn execute_swap_action(
    fiber_agent_ref: CchFiberAgentRef,
    cch_actor_ref: ActorRef<CchMessage>,
    order: &SwapOrder,
    action: SwapOrderAction,
    retry_count: u32,
    fallback_delta_seconds: u64,
) -> Result<()> {
    match action {
        SwapOrderAction::SendQuotePayment => {
            let Some(max_outgoing_seconds) =
                SwapOutgoingPaymentDispatcher::max_outgoing_expiry_seconds(
                    order,
                    SwapOutgoingPaymentDispatcher::now_secs(),
                    fallback_delta_seconds,
                )
            else {
                anyhow::bail!("insufficient remaining time for quote leg");
            };
            let executor = SwapOutgoingPaymentDispatcher::build_executor(
                fiber_agent_ref,
                cch_actor_ref,
                order,
                max_outgoing_seconds
                    .saturating_mul(1000)
                    .min(MAX_PAYMENT_TLC_EXPIRY_LIMIT),
                retry_count,
            );
            use super::ActionExecutor;
            Box::new(executor).execute().await
        }
        SwapOrderAction::SettleBaseInvoice => {
            let preimage = order.payment_preimage.ok_or_else(|| {
                anyhow::anyhow!("cannot settle base leg without preimage")
            })?;
            let executor = SettleFiberIncomingInvoiceExecutor::new(
                order.payment_hash,
                preimage,
                cch_actor_ref,
                fiber_agent_ref,
            );
            use super::ActionExecutor;
            Box::new(executor).execute().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::InvoiceBuilder;
    use ckb_types::packed::Script;
    use ckb_types::prelude::{Builder, Entity};
    use fiber_types::invoice::Currency;
    use fiber_types::Hash256;

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

    fn make_order(status: CchOrderStatus) -> SwapOrder {
        let mut o = SwapOrder::new(
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
        .unwrap();
        o.status = status;
        o
    }

    #[test]
    fn swap_on_entering_maps_status_to_actions() {
        assert!(swap_on_entering(&make_order(CchOrderStatus::Pending)).is_empty());
        assert_eq!(
            swap_on_entering(&make_order(CchOrderStatus::IncomingAccepted)),
            vec![SwapOrderAction::SendQuotePayment]
        );
        assert!(swap_on_entering(&make_order(CchOrderStatus::OutgoingInFlight)).is_empty());
        assert_eq!(
            swap_on_entering(&make_order(CchOrderStatus::OutgoingSuccess)),
            vec![SwapOrderAction::SettleBaseInvoice]
        );
        assert!(swap_on_entering(&make_order(CchOrderStatus::Success)).is_empty());
        assert!(swap_on_entering(&make_order(CchOrderStatus::Failed)).is_empty());
    }

}
