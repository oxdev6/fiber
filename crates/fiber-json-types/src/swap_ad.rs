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
