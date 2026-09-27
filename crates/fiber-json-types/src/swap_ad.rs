use ckb_jsonrpc_types::Script;
use crate::serde_utils::{Hash256, Pubkey, U128Hex, U64Hex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

/// Parameters for publishing a swap advertisement.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PublishSwapAdParams {
    /// Base UDT script (e.g., RGB++ xUDT). None for CKB native.
    pub base_udt_script: Option<Script>,
    /// Quote UDT script (target CKB UDT). None for CKB native.
    pub quote_udt_script: Option<Script>,
    /// Rate: quote per base * 1e8.
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub rate: u128,
    /// Fee rate: proportional millionths.
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub fee_rate: u128,
    /// Expiry delta in milliseconds from now.
    #[serde_as(as = "U64Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub expiry_delta_millis: u64,
}

/// Parameters for listing swap advertisements.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListSwapAdsParams {
    /// Filter by base script (exact match). None = no filter.
    pub base_udt_script: Option<Script>,
    /// Filter by quote script (exact match). None = no filter.
    pub quote_udt_script: Option<Script>,
}

/// Parameters for getting best swap rate.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetBestSwapRateParams {
    /// Base script to filter.
    pub base_udt_script: Option<Script>,
    /// Quote script to filter.
    pub quote_udt_script: Option<Script>,
}

/// View of a swap advertisement for RPC response.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SwapOffer {
    /// Base UDT script.
    pub base_udt_script: Option<Script>,
    /// Quote UDT script.
    pub quote_udt_script: Option<Script>,
    /// Rate as string and raw.
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub rate: u128,
    /// Fee rate.
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub fee_rate: u128,
    /// Operator pubkey.
    pub operator_pubkey: Pubkey,
    /// Broadcast timestamp.
    #[serde_as(as = "U64Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub timestamp: u64,
    /// Expiry timestamp.
    #[serde_as(as = "U64Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub expiry: u64,
    /// Chain hash.
    pub chain_hash: Hash256,
    /// Whether expired at query time.
    pub is_expired: bool,
}

/// Result for list.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListSwapAdsResult {
    pub offers: Vec<SwapOffer>,
}

/// Result for best rate.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetBestSwapRateResult {
    pub offer: Option<SwapOffer>,
}

/// Parameters for executing a swap against a live advertisement.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteSwapParams {
    /// Hash of the swap advertisement to execute (from list output).
    pub ad_hash: Hash256,
    /// Base-asset units the user will pay.
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub base_amount: u128,
    /// User's quote-asset invoice (holds the preimage) as a string.
    pub quote_pay_req: String,
}

/// A Fiber-to-Fiber swap order.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SwapOrderResponse {
    /// Payment hash linking both legs (also the order id).
    pub payment_hash: Hash256,
    /// Advertisement hash this order executes.
    pub ad_hash: Hash256,
    /// Hub's base-asset invoice for the user to pay, as a string.
    pub base_pay_req: String,
    /// User's quote-asset invoice the hub pays, as given.
    pub quote_pay_req: String,
    /// Base units in.
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub base_amount: u128,
    /// Quote units out (frozen at creation).
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub quote_amount: u128,
    /// Frozen rate (quote per base × 1e8).
    #[serde_as(as = "U128Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub rate: u128,
    /// Order status.
    pub status: crate::cch::CchOrderStatus,
    /// Seconds since epoch when created.
    #[serde_as(as = "U64Hex")]
    #[schemars(schema_with = "crate::schema_helpers::schema_as_uint_hex")]
    pub created_at: u64,
}
