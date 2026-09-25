//! Unit tests for CchOrderStateMachine
//!
//! Tests cover:
//! - Valid state transitions
//! - Invalid state transitions
//! - on_entering actions for each status
//! - Failure transitions from active states

use crate::cch::order::{state_machine::CchOrderEvent, CchOrderStateMachine};
use crate::cch::CchError;
use crate::invoice::CkbInvoiceStatus;
use fiber_types::{CchInvoice, CchOrder, CchOrderStatus};
use fiber_types::{Hash256, PaymentStatus};

/// Helper function to create a test payment hash
fn test_payment_hash(value: u8) -> Hash256 {
    let mut bytes = [0u8; 32];
    bytes[0] = value;
    Hash256::from(bytes)
}

/// Helper function to create a valid preimage/payment hash pair.
/// The preimage will hash to the payment hash using SHA256.
fn create_valid_preimage_pair(seed: u8) -> (Hash256, Hash256) {
    use fiber_types::HashAlgorithm;
    // Generate a preimage from the seed
    let mut preimage_bytes = [0u8; 32];
    preimage_bytes[0] = seed;
    preimage_bytes[1] = seed.wrapping_mul(2);
    preimage_bytes[2] = seed.wrapping_add(1);
    let preimage = Hash256::from(preimage_bytes);

    // Compute the payment hash from the preimage
    let hash_algorithm = HashAlgorithm::Sha256;
    let payment_hash = Hash256::from(hash_algorithm.hash(preimage));

    (preimage, payment_hash)
}

/// Helper function to create a test CchOrder with configurable status
fn create_test_order(status: CchOrderStatus) -> CchOrder {
    create_test_order_with_payment_hash(status, test_payment_hash(1))
}

/// Helper function to create a test CchOrder with configurable status and payment hash
fn create_test_order_with_payment_hash(status: CchOrderStatus, payment_hash: Hash256) -> CchOrder {
    // Create a minimal valid Lightning invoice string for testing
    // This is a mainnet invoice format that parses correctly
    let btc_invoice_str = "lnbc1pvjluezsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygspp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdpl2pkx2ctnv5sxxmmwwd5kgetjypeh2ursdae8g6twvus8g6rfwvs8qun0dfjkxaq9qrsgq357wnc5r2ueh7ck6q93dj32dlqnls087fxdwk8qakdyafkq3yap9us6v52vjjsrvywa6rt52cm9r9zqt8r2t7mlcwspyetp5h2tztugp9lfyql";
    let btc_invoice: lightning_invoice::Bolt11Invoice = btc_invoice_str.parse().unwrap();

    CchOrder {
        created_at: 1700000000,
        expiry_delta_seconds: 3600,
        wrapped_btc_type_script: ckb_jsonrpc_types::Script {
            code_hash: Default::default(),
            hash_type: ckb_jsonrpc_types::ScriptHashType::Data,
            args: Default::default(),
        },
        outgoing_pay_req: btc_invoice_str.to_string(),
        incoming_invoice: CchInvoice::Lightning(btc_invoice),
        payment_hash,
        payment_preimage: None,
        amount_sats: 100000,
        fee_sats: 100,
        status,
        failure_reason: None,
    }
}

// ============================================================================
// Tests for valid state transitions via invoice events
// ============================================================================

#[test]
fn test_transition_pending_to_incoming_accepted_via_invoice_received() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Received,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::IncomingAccepted);
}

#[test]
fn test_transition_pending_to_failed_via_invoice_cancelled() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Cancelled,
        failure_reason: Some("cancelled by user".to_string()),
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
    assert_eq!(order.failure_reason, Some("cancelled by user".to_string()));
}

#[test]
fn test_transition_pending_to_failed_via_invoice_expired() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Expired,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
    assert!(order.failure_reason.is_some());
}

#[test]
fn test_transition_outgoing_success_to_success_via_invoice_paid() {
    let mut order = create_test_order(CchOrderStatus::OutgoingSuccess);
    order.payment_preimage = Some(test_payment_hash(99));

    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Paid,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Success);
}

// ============================================================================
// Tests for valid state transitions via payment events
// ============================================================================

#[test]
fn test_transition_incoming_accepted_to_outgoing_in_flight_via_payment_inflight() {
    let mut order = create_test_order(CchOrderStatus::IncomingAccepted);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Inflight,
        payment_preimage: None,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::OutgoingInFlight);
}

#[test]
fn test_transition_incoming_accepted_to_outgoing_succeeded_via_payment_success() {
    let (preimage, payment_hash) = create_valid_preimage_pair(42);
    let mut order =
        create_test_order_with_payment_hash(CchOrderStatus::IncomingAccepted, payment_hash);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Success,
        payment_preimage: Some(preimage),
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::OutgoingSuccess);
    assert_eq!(order.payment_preimage, Some(preimage));
}

#[test]
fn test_transition_outgoing_in_flight_to_outgoing_succeeded_via_payment_success() {
    let (preimage, payment_hash) = create_valid_preimage_pair(42);
    let mut order =
        create_test_order_with_payment_hash(CchOrderStatus::OutgoingInFlight, payment_hash);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Success,
        payment_preimage: Some(preimage),
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::OutgoingSuccess);
    assert_eq!(order.payment_preimage, Some(preimage));
}

#[test]
fn test_stale_payment_created_after_outgoing_in_flight_is_ignored() {
    let mut order = create_test_order(CchOrderStatus::OutgoingInFlight);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Created,
        payment_preimage: None,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_none());
    assert_eq!(order.status, CchOrderStatus::OutgoingInFlight);
}

#[test]
fn test_transition_incoming_accepted_to_failed_via_payment_failed() {
    let mut order = create_test_order(CchOrderStatus::IncomingAccepted);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Failed,
        payment_preimage: None,
        failure_reason: Some("no route found".to_string()),
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
    assert_eq!(order.failure_reason, Some("no route found".to_string()));
}

#[test]
fn test_transition_outgoing_in_flight_to_failed_via_payment_failed() {
    let mut order = create_test_order(CchOrderStatus::OutgoingInFlight);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Failed,
        payment_preimage: None,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
    assert!(order.failure_reason.is_some());
}

// ============================================================================
// Tests for staying in same status (no-op transitions)
// ============================================================================

#[test]
fn test_staying_in_pending_via_invoice_open() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Open,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_none());
    assert_eq!(order.status, CchOrderStatus::Pending);
}

#[test]
fn test_staying_in_incoming_accepted_via_payment_created() {
    let mut order = create_test_order(CchOrderStatus::IncomingAccepted);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Created,
        payment_preimage: None,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_none());
    assert_eq!(order.status, CchOrderStatus::IncomingAccepted);
}

#[test]
fn test_staying_in_outgoing_in_flight_via_payment_inflight() {
    let mut order = create_test_order(CchOrderStatus::OutgoingInFlight);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Inflight,
        payment_preimage: None,
        failure_reason: None,
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_none());
    assert_eq!(order.status, CchOrderStatus::OutgoingInFlight);
}

// ============================================================================
// Tests for invalid state transitions
// ============================================================================

#[test]
fn test_invalid_transition_pending_rejects_outgoing_payment_created() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Created,
        payment_preimage: None,
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(
        result,
        Err(CchError::InvalidTransition(
            CchOrderStatus::Pending,
            CchOrderStatus::IncomingAccepted
        ))
    ));
    assert_eq!(order.status, CchOrderStatus::Pending);
}

#[test]
fn test_invalid_transition_pending_to_outgoing_in_flight() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Inflight,
        payment_preimage: None,
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(result, Err(CchError::InvalidTransition(_, _))));
    assert_eq!(order.status, CchOrderStatus::Pending);
}

#[test]
fn test_invalid_transition_pending_to_outgoing_succeeded() {
    let (preimage, payment_hash) = create_valid_preimage_pair(42);
    let mut order = create_test_order_with_payment_hash(CchOrderStatus::Pending, payment_hash);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Success,
        payment_preimage: Some(preimage),
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(result, Err(CchError::InvalidTransition(_, _))));
    assert_eq!(order.status, CchOrderStatus::Pending);
    assert_eq!(order.payment_preimage, None);
}

#[test]
fn test_invalid_transition_pending_rejects_outgoing_payment_failed() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Failed,
        payment_preimage: None,
        failure_reason: Some("test failure".to_string()),
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(
        result,
        Err(CchError::InvalidTransition(
            CchOrderStatus::Pending,
            CchOrderStatus::Failed
        ))
    ));
    assert_eq!(order.status, CchOrderStatus::Pending);
    assert_eq!(order.failure_reason, None);
}

#[test]
fn test_invalid_transition_pending_to_success() {
    let mut order = create_test_order(CchOrderStatus::Pending);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Paid,
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(result, Err(CchError::InvalidTransition(_, _))));
}

#[test]
fn test_invalid_transition_success_to_any_other() {
    let mut order = create_test_order(CchOrderStatus::Success);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Open,
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(result, Err(CchError::InvalidTransition(_, _))));
}

#[test]
fn test_invalid_transition_failed_to_any_other() {
    let mut order = create_test_order(CchOrderStatus::Failed);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Open,
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(result, Err(CchError::InvalidTransition(_, _))));
}

// ============================================================================
// Tests for error conditions
// ============================================================================

#[test]
fn test_payment_success_without_preimage_returns_error() {
    let mut order = create_test_order(CchOrderStatus::IncomingAccepted);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Success,
        payment_preimage: None, // Missing preimage!
        failure_reason: None,
    };

    let result = CchOrderStateMachine::apply(&mut order, event);

    assert!(matches!(
        result,
        Err(CchError::SettledPaymentMissingPreimage)
    ));
}

// ============================================================================
// Tests for failure transitions from active states
// ============================================================================

#[test]
fn test_failure_from_incoming_accepted() {
    let mut order = create_test_order(CchOrderStatus::IncomingAccepted);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Cancelled,
        failure_reason: Some("test failure".to_string()),
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
}

#[test]
fn test_failure_from_outgoing_in_flight() {
    let mut order = create_test_order(CchOrderStatus::OutgoingInFlight);
    let event = CchOrderEvent::OutgoingPaymentChanged {
        status: PaymentStatus::Failed,
        payment_preimage: None,
        failure_reason: Some("test failure".to_string()),
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
}

#[test]
fn test_failure_from_outgoing_succeeded() {
    let mut order = create_test_order(CchOrderStatus::OutgoingSuccess);
    let event = CchOrderEvent::IncomingInvoiceChanged {
        status: CkbInvoiceStatus::Cancelled,
        failure_reason: Some("test failure".to_string()),
    };

    let transition = CchOrderStateMachine::apply(&mut order, event).unwrap();

    assert!(transition.is_some());
    assert_eq!(order.status, CchOrderStatus::Failed);
}

// ============================================================================
// Tests for is_final
// ============================================================================

#[test]
fn test_is_final_returns_true_for_success() {
    let order = create_test_order(CchOrderStatus::Success);
    assert!(order.is_final());
}

#[test]
fn test_is_final_returns_true_for_failed() {
    let order = create_test_order(CchOrderStatus::Failed);
    assert!(order.is_final());
}

#[test]
fn test_is_final_returns_false_for_pending() {
    let order = create_test_order(CchOrderStatus::Pending);
    assert!(!order.is_final());
}

#[test]
fn test_is_final_returns_false_for_in_progress_statuses() {
    for status in [
        CchOrderStatus::IncomingAccepted,
        CchOrderStatus::OutgoingInFlight,
        CchOrderStatus::OutgoingSuccess,
    ] {
        let order = create_test_order(status);
        assert!(!order.is_final(), "Expected {:?} to not be final", status);
    }
}

/// Helper to build a minimal signed test invoice for swap orders.
fn test_swap_invoice() -> fiber_types::CkbInvoice {
    use crate::invoice::InvoiceBuilder;
    use fiber_types::Currency;
    use secp256k1::SECP256K1;
    let secret = secp256k1::SecretKey::from_slice(&[42u8; 32]).unwrap();
    InvoiceBuilder::new(Currency::Fibd)
        .amount(Some(100))
        .payment_hash(test_payment_hash(9))
        .build_with_sign(|msg| SECP256K1.sign_ecdsa_recoverable(msg, &secret))
        .unwrap()
}

fn dummy_swap_script(byte: u8) -> ckb_types::packed::Script {
    use ckb_types::prelude::{Builder, Entity};
    let mut h = [0u8; 32];
    h[0] = byte;
    ckb_types::packed::Script::new_builder()
        .code_hash(ckb_types::packed::Byte32::from_slice(&h).unwrap())
        .hash_type(ckb_types::core::ScriptHashType::Type)
        .args(ckb_types::packed::Bytes::from(vec![byte]))
        .build()
}

/// Helper to create a test SwapOrder whose payment hash matches `preimage`.
fn create_test_swap_order(
    preimage: Hash256,
    payment_hash: Hash256,
) -> fiber_types::SwapOrder {
    fiber_types::SwapOrder::new(
        1_700_000_000,
        3_600,
        Hash256::default(),
        Some(dummy_swap_script(1)),
        Some(dummy_swap_script(2)),
        10_000_000_000,
        3_000,
        100,
        payment_hash,
        test_swap_invoice(),
        "fiber-pay-req-placeholder".to_string(),
    )
    .unwrap()
}

#[test]
fn test_swap_order_full_lifecycle_through_shared_machine() {
    use crate::cch::order::state_machine::CchOrderEvent;

    let (preimage, payment_hash) = create_valid_preimage_pair(7);
    let mut order = create_test_swap_order(preimage, payment_hash);
    assert_eq!(order.status, CchOrderStatus::Pending);

    // Leg progress: incoming invoice received.
    let next = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::IncomingInvoiceChanged {
            status: CkbInvoiceStatus::Received,
            failure_reason: None,
        },
    )
    .unwrap();
    assert_eq!(next, Some(CchOrderStatus::IncomingAccepted));

    // Outgoing leg in flight.
    let next = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::OutgoingPaymentChanged {
            status: PaymentStatus::Inflight,
            payment_preimage: None,
            failure_reason: None,
        },
    )
    .unwrap();
    assert_eq!(next, Some(CchOrderStatus::OutgoingInFlight));

    // Outgoing settles with preimage reveal.
    let next = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::OutgoingPaymentChanged {
            status: PaymentStatus::Success,
            payment_preimage: Some(preimage),
            failure_reason: None,
        },
    )
    .unwrap();
    assert_eq!(next, Some(CchOrderStatus::OutgoingSuccess));
    assert_eq!(order.payment_preimage, Some(preimage));

    // Incoming leg settles -> order success.
    let next = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::IncomingInvoiceChanged {
            status: CkbInvoiceStatus::Paid,
            failure_reason: None,
        },
    )
    .unwrap();
    assert_eq!(next, Some(CchOrderStatus::Success));
    assert!(order.failure_reason.is_none());
}

#[test]
fn test_swap_order_rejects_bad_transitions_like_cch() {
    use crate::cch::order::state_machine::CchOrderEvent;

    let (preimage, payment_hash) = create_valid_preimage_pair(8);
    let mut order = create_test_swap_order(preimage, payment_hash);

    // Outgoing event on a Pending order is rejected, same as CchOrder.
    let err = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::OutgoingPaymentChanged {
            status: PaymentStatus::Inflight,
            payment_preimage: None,
            failure_reason: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CchError::InvalidTransition(_, _)));

    // Wrong preimage is rejected.
    let _ = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::IncomingInvoiceChanged {
            status: CkbInvoiceStatus::Received,
            failure_reason: None,
        },
    )
    .unwrap();
    let err = CchOrderStateMachine::apply_to(
        &mut order,
        CchOrderEvent::OutgoingPaymentChanged {
            status: PaymentStatus::Success,
            payment_preimage: Some(test_payment_hash(99)),
            failure_reason: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CchError::PreimageHashMismatch));
}
