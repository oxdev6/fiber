use ckb_types::packed::Script;

use fiber_types::{Hash256, SwapAdvertisement};

use crate::fiber::gossip::GossipMessageStore;
use serde_with::serde_as;
use fiber_types::serde_utils::EntityHex;

/// Filter for listing swap advertisements.
/// W3 minimal: exact match on Option<Script> (code_hash + hash_type + args).
#[serde_as]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AssetPairFilter {
    #[serde_as(as = "Option<EntityHex>")]
    pub base_udt_script: Option<Script>,
    #[serde_as(as = "Option<EntityHex>")]
    pub quote_udt_script: Option<Script>,
}

impl AssetPairFilter {
    pub fn matches(&self, ad: &SwapAdvertisement) -> bool {
        if let Some(ref base) = self.base_udt_script {
            if ad.base_udt_script.as_ref() != Some(base) {
                return false;
            }
        }
        if let Some(ref quote) = self.quote_udt_script {
            if ad.quote_udt_script.as_ref() != Some(quote) {
                return false;
            }
        }
        true
    }
}

/// Canonical content hash identifying an advertisement.
/// Matches the `BroadcastMessageID::SwapAdvertisement` used by the store.
pub fn swap_ad_hash(ad: &SwapAdvertisement) -> Hash256 {
    let h = ckb_hash::blake2b_256(ad.message_to_sign());
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&h[..32]);
    Hash256::from(arr)
}

/// View type for RPC (hex for scripts, string for rate).
#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SwapOfferView {
    /// Content hash — pass back as `ad_hash` to execute this offer.
    pub ad_hash: Hash256,
    #[serde_as(as = "Option<EntityHex>")]
    pub base_udt_script: Option<Script>,
    #[serde_as(as = "Option<EntityHex>")]
    pub quote_udt_script: Option<Script>,
    pub rate: String,
    pub rate_raw: u128,
    pub fee_rate: String,
    pub fee_rate_raw: u128,
    pub operator_pubkey: fiber_types::Pubkey,
    pub timestamp: u64,
    pub expiry: u64,
    pub chain_hash: Hash256,
    pub is_expired: bool,
}

impl From<SwapAdvertisement> for SwapOfferView {
    fn from(ad: SwapAdvertisement) -> Self {
        let now = crate::now_timestamp_as_millis_u64();
        Self {
            ad_hash: swap_ad_hash(&ad),
            base_udt_script: ad.base_udt_script.clone(),
            quote_udt_script: ad.quote_udt_script.clone(),
            rate: ad.rate.to_string(),
            rate_raw: ad.rate,
            fee_rate: ad.fee_rate.to_string(),
            fee_rate_raw: ad.fee_rate,
            operator_pubkey: ad.operator_pubkey,
            timestamp: ad.timestamp,
            expiry: ad.expiry,
            chain_hash: ad.chain_hash,
            is_expired: ad.is_expired(now),
        }
    }
}

/// In-memory index that queries store directly each call (W3 minimal, no background subscription).
pub struct SwapAdIndex<S: GossipMessageStore> {
    store: S,
}

impl<S: GossipMessageStore> SwapAdIndex<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// List active offers, optionally filtered by pair.
    pub fn list_offers(
        &self,
        filter: Option<AssetPairFilter>,
        now: u64,
    ) -> Vec<SwapAdvertisement> {
        let mut offers = self.store.get_active_swap_advertisements(now);
        if let Some(f) = filter {
            offers.retain(|ad| f.matches(ad));
        }
        // Sort by rate descending (best rate first), then by timestamp descending.
        offers.sort_by(|a, b| b.rate.cmp(&a.rate).then(b.timestamp.cmp(&a.timestamp)));
        offers
    }

    /// Best rate for exact pair (max rate).
    pub fn best_rate(
        &self,
        base: &Option<Script>,
        quote: &Option<Script>,
        now: u64,
    ) -> Option<SwapAdvertisement> {
        let filter = AssetPairFilter {
            base_udt_script: base.clone(),
            quote_udt_script: quote.clone(),
        };
        self.list_offers(Some(filter), now).into_iter().next()
    }

    /// List with cursor pagination (for future).
    pub fn list_with_cursor(
        &self,
        after_cursor: Option<&fiber_types::Cursor>,
        limit: usize,
        now: u64,
    ) -> Vec<(fiber_types::Cursor, SwapAdvertisement)> {
        self.store
            .get_swap_advertisements(after_cursor, limit)
            .into_iter()
            .filter(|(_, ad)| !ad.is_expired(now))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_types::packed::Script;
    use ckb_types::prelude::{Builder, Entity, Pack};
    use fiber_types::{Hash256, Privkey, SwapAdvertisement};
    use crate::fiber::gossip::GossipMessageStore;
    use crate::tests::test_utils::generate_store;

    fn dummy_script(byte: u8) -> Script {
        let code_hash = {
            let mut h = [0u8; 32];
            h[0] = byte;
            ckb_types::packed::Byte32::from_slice(&h).unwrap()
        };
        Script::new_builder()
            .code_hash(code_hash)
            .hash_type(ckb_types::core::ScriptHashType::Type)
            .args(vec![byte].pack())
            .build()
    }

    fn make_ad(privkey: &Privkey, base: Option<Script>, quote: Option<Script>, rate: u128, ts: u64, expiry: u64) -> SwapAdvertisement {
        SwapAdvertisement::new_signed(
            Hash256::default(),
            base,
            quote,
            rate,
            3000,
            privkey,
            ts,
            expiry,
        )
    }

    #[test]
    fn test_list_offers_filters_expired() {
        let (store, _dir) = generate_store();
        let privkey = Privkey::from_slice(&[42u8; 32]);
        let now = 1500;
        let ad_active = make_ad(&privkey, Some(dummy_script(1)), Some(dummy_script(2)), 100, 1000, 2000);
        let ad_expired = make_ad(&privkey, Some(dummy_script(1)), Some(dummy_script(2)), 200, 500, 1000);
        store.save_swap_advertisement(ad_active.clone());
        store.save_swap_advertisement(ad_expired.clone());
        let index = SwapAdIndex::new(store);
        let offers = index.list_offers(None, now);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].rate, 100);
    }

    #[test]
    fn test_best_rate_picks_max() {
        let (store, _dir) = generate_store();
        let privkey = Privkey::from_slice(&[42u8; 32]);
        let now = 1500;
        let ad_low = make_ad(&privkey, Some(dummy_script(1)), Some(dummy_script(2)), 100, 1000, 3000);
        let ad_high = make_ad(&privkey, Some(dummy_script(1)), Some(dummy_script(2)), 300, 1000, 3000);
        // Need different operator or timestamp to have different hash
        let privkey2 = Privkey::from_slice(&[43u8; 32]);
        let ad_high2 = make_ad(&privkey2, Some(dummy_script(1)), Some(dummy_script(2)), 300, 1100, 3100);
        store.save_swap_advertisement(ad_low.clone());
        store.save_swap_advertisement(ad_high.clone());
        store.save_swap_advertisement(ad_high2.clone());
        let index = SwapAdIndex::new(store);
        let best = index.best_rate(&Some(dummy_script(1)), &Some(dummy_script(2)), now).unwrap();
        assert_eq!(best.rate, 300);
    }

    #[test]
    fn test_store_list_after_cursor() {
        let (store, _dir) = generate_store();
        let privkey = Privkey::from_slice(&[42u8; 32]);
        let privkey2 = Privkey::from_slice(&[43u8; 32]);
        let _now = 5000;
        let ad1 = make_ad(&privkey, None, None, 100, 1000, 4000);
        let ad2 = make_ad(&privkey2, None, None, 200, 2000, 5000);
        store.save_swap_advertisement(ad1.clone());
        store.save_swap_advertisement(ad2.clone());
        let after = ad1.cursor();
        let list = store.get_swap_advertisements(Some(&after), 10);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1.rate, 200);
    }
}
