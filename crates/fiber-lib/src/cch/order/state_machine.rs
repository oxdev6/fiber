use crate::{
    cch::{trackers::CchTrackingEvent, CchError},
    invoice::CkbInvoiceStatus,
};
use fiber_types::HashAlgorithm;
use fiber_types::{payment::PaymentStatus, Hash256};
use fiber_types::{CchOrder, CchOrderStatus, SwapOrder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CchOrderEvent {
    IncomingInvoiceChanged {
        status: CkbInvoiceStatus,
        failure_reason: Option<String>,
    },
    OutgoingPaymentChanged {
        status: PaymentStatus,
        payment_preimage: Option<Hash256>,
        failure_reason: Option<String>,
    },
}

pub struct CchOrderStateMachine;

/// Minimal order surface the state machine drives.
///
/// Implemented by [`CchOrder`] (original) and [`SwapOrder`](fiber_types::SwapOrder)
/// (Fiber→Fiber swaps) so both share one transition table with zero forks.
pub trait OrderStatusHolder {
    fn status(&self) -> CchOrderStatus;
    fn set_status(&mut self, status: CchOrderStatus);
    fn payment_hash(&self) -> &Hash256;
    fn payment_preimage(&self) -> &Option<Hash256>;
    fn set_payment_preimage(&mut self, preimage: Option<Hash256>);
    fn set_failure_reason(&mut self, reason: Option<String>);
}

impl OrderStatusHolder for CchOrder {
    fn status(&self) -> CchOrderStatus {
        self.status
    }
    fn set_status(&mut self, status: CchOrderStatus) {
        self.status = status;
    }
    fn payment_hash(&self) -> &Hash256 {
        &self.payment_hash
    }
    fn payment_preimage(&self) -> &Option<Hash256> {
        &self.payment_preimage
    }
    fn set_payment_preimage(&mut self, preimage: Option<Hash256>) {
        self.payment_preimage = preimage;
    }
    fn set_failure_reason(&mut self, reason: Option<String>) {
        self.failure_reason = reason;
    }
}

impl CchOrderStateMachine {
    /// Apply an event to the order and return the transition.
    ///
    /// Returns `Ok(Some(new_status))` if the order is transit to a new status.
    /// or `Ok(None)` if the order stays in the same status without changes.
    pub fn apply(
        order: &mut CchOrder,
        event: CchOrderEvent,
    ) -> Result<Option<CchOrderStatus>, CchError> {
        Self::apply_to(order, event)
    }

    /// Generic entry point shared by [`CchOrder`] and [`SwapOrder`](fiber_types::SwapOrder).
    pub fn apply_to(
        order: &mut impl OrderStatusHolder,
        event: CchOrderEvent,
    ) -> Result<Option<CchOrderStatus>, CchError> {
        match event {
            CchOrderEvent::IncomingInvoiceChanged {
                status,
                failure_reason,
            } => Self::try_transite_to(order, status.into(), move || {
                failure_reason.unwrap_or_else(|| format!("incoming invoice failed: {}", status))
            }),
            CchOrderEvent::OutgoingPaymentChanged {
                status,
                payment_preimage,
                failure_reason,
            } => {
                let to = status.into();
                if order.status() == CchOrderStatus::Pending {
                    // A pending order can only be accepted by the incoming invoice tracker.
                    return Err(CchError::InvalidTransition(order.status(), to));
                }
                if status == PaymentStatus::Created
                    && matches!(
                        order.status(),
                        CchOrderStatus::OutgoingInFlight | CchOrderStatus::OutgoingSuccess
                    )
                {
                    return Ok(None);
                }
                if status == PaymentStatus::Success && payment_preimage.is_none() {
                    return Err(CchError::SettledPaymentMissingPreimage);
                }
                // Verify preimage hashes to payment_hash if provided
                if let Some(ref preimage) = payment_preimage {
                    let hash_algorithm = HashAlgorithm::Sha256;
                    let computed_hash = hash_algorithm.hash(*preimage);
                    if computed_hash.as_slice() != order.payment_hash().as_ref() {
                        return Err(CchError::PreimageHashMismatch);
                    }
                }
                let new_status = Self::try_transite_to(order, to, move || {
                    failure_reason
                        .unwrap_or_else(|| format!("outgoing payment failed: {:?}", status))
                })?;
                if new_status.is_some() && order.payment_preimage().is_none() {
                    order.set_payment_preimage(payment_preimage);
                }
                Ok(new_status)
            }
        }
    }

    fn allow_transition(from: CchOrderStatus, to: CchOrderStatus) -> bool {
        match (from, to) {
            (CchOrderStatus::Pending, CchOrderStatus::IncomingAccepted) => true,
            // When the payment succeeds immediately, we can transit to the `OutgoingSuccess` directly.
            (
                CchOrderStatus::IncomingAccepted,
                CchOrderStatus::OutgoingInFlight | CchOrderStatus::OutgoingSuccess,
            ) => true,
            (CchOrderStatus::OutgoingInFlight, CchOrderStatus::OutgoingSuccess) => true,
            (CchOrderStatus::OutgoingSuccess, CchOrderStatus::Success) => true,
            (_, CchOrderStatus::Failed) if from != CchOrderStatus::Success => true,
            _ => {
                // Allow staying in the same status
                from == to
            }
        }
    }

    fn try_transite_to<F>(
        order: &mut impl OrderStatusHolder,
        to: CchOrderStatus,
        failure_reason_fn: F,
    ) -> Result<Option<CchOrderStatus>, CchError>
    where
        F: FnOnce() -> String,
    {
        if !Self::allow_transition(order.status(), to) {
            return Err(CchError::InvalidTransition(order.status(), to));
        }
        if order.status() != to {
            order.set_status(to);
            if to == CchOrderStatus::Failed {
                order.set_failure_reason(Some(failure_reason_fn()));
            }
            Ok(Some(to))
        } else {
            Ok(None)
        }
    }
}

impl OrderStatusHolder for SwapOrder {
    fn status(&self) -> CchOrderStatus {
        self.status
    }
    fn set_status(&mut self, status: CchOrderStatus) {
        self.status = status;
    }
    fn payment_hash(&self) -> &Hash256 {
        &self.payment_hash
    }
    fn payment_preimage(&self) -> &Option<Hash256> {
        &self.payment_preimage
    }
    fn set_payment_preimage(&mut self, preimage: Option<Hash256>) {
        self.payment_preimage = preimage;
    }
    fn set_failure_reason(&mut self, reason: Option<String>) {
        self.failure_reason = reason;
    }
}

impl From<CchTrackingEvent> for CchOrderEvent {
    fn from(event: CchTrackingEvent) -> Self {
        match event {
            CchTrackingEvent::InvoiceChanged {
                payment_hash: _,
                status,
                failure_reason,
            } => CchOrderEvent::IncomingInvoiceChanged {
                status,
                failure_reason,
            },
            CchTrackingEvent::PaymentChanged {
                payment_hash: _,
                status,
                payment_preimage,
                failure_reason,
            } => CchOrderEvent::OutgoingPaymentChanged {
                status,
                payment_preimage,
                failure_reason,
            },
        }
    }
}
