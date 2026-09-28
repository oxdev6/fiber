use crate::fiber::gossip::GossipMessageStore;
use crate::fiber::network::{NetworkActorCommand, NetworkActorMessage};
use crate::fiber::swap_advertisement::{AssetPairFilter, SwapAdIndex};
use crate::fiber::types::BroadcastMessageWithTimestamp;
use fiber_types::{Hash256, SwapAdvertisement};
use jsonrpsee::{proc_macros::rpc, types::ErrorObjectOwned};
use ckb_types::packed::Script;
use ractor::ActorRef;
use fiber_json_types::swap_ad::{
    PublishSwapAdParams, ListSwapAdsParams, GetBestSwapRateParams, SwapOffer, ListSwapAdsResult, GetBestSwapRateResult,
};

/// RPC for swap advertisement market.
#[rpc(server)]
pub trait SwapAdRpc {
    /// Publish a swap advertisement (hub). Signs with node's secret key.
    #[method(name = "publish_swap_advertisement")]
    async fn publish_swap_advertisement(
        &self,
        params: PublishSwapAdParams,
    ) -> Result<SwapOffer, ErrorObjectOwned>;

    /// List active swap advertisements, optionally filtered by pair.
    #[method(name = "list_swap_advertisements")]
    async fn list_swap_advertisements(
        &self,
        params: ListSwapAdsParams,
    ) -> Result<ListSwapAdsResult, ErrorObjectOwned>;

    /// Get best rate for exact pair (max rate).
    #[method(name = "get_best_swap_rate")]
    async fn get_best_swap_rate(
        &self,
        params: GetBestSwapRateParams,
    ) -> Result<GetBestSwapRateResult, ErrorObjectOwned>;
}

pub struct SwapAdRpcServerImpl<S>
where
    S: GossipMessageStore + Clone + Send + Sync + 'static,
{
    store: S,
    ckb_chain_hash: Option<Hash256>,
    ckb_config: Option<crate::ckb::CkbConfig>,
    network_actor: Option<ActorRef<NetworkActorMessage>>,
}

impl<S> SwapAdRpcServerImpl<S>
where
    S: GossipMessageStore + Clone + Send + Sync + 'static,
{
    pub fn new(
        store: S,
        ckb_config: Option<crate::ckb::CkbConfig>,
        ckb_chain_hash: Option<Hash256>,
        network_actor: Option<ActorRef<NetworkActorMessage>>,
    ) -> Self {
        Self {
            store,
            ckb_config,
            ckb_chain_hash,
            network_actor,
        }
    }
}

fn json_script_to_packed(s: ckb_jsonrpc_types::Script) -> Script {
    s.into()
}

fn packed_to_json_script(s: Script) -> ckb_jsonrpc_types::Script {
    s.into()
}

fn ad_to_offer(ad: SwapAdvertisement) -> SwapOffer {
    use crate::fiber::swap_advertisement::swap_ad_hash;
    let now = crate::now_timestamp_as_millis_u64();
    SwapOffer {
        ad_hash: swap_ad_hash(&ad).into(),
        base_udt_script: ad.base_udt_script.clone().map(|s| s.into()),
        quote_udt_script: ad.quote_udt_script.clone().map(|s| s.into()),
        rate: ad.rate,
        fee_rate: ad.fee_rate,
        operator_pubkey: ad.operator_pubkey.into(),
        timestamp: ad.timestamp,
        expiry: ad.expiry,
        chain_hash: ad.chain_hash.into(),
        is_expired: ad.is_expired(now),
    }
}

#[async_trait::async_trait]
impl<S> SwapAdRpcServer for SwapAdRpcServerImpl<S>
where
    S: GossipMessageStore + Clone + Send + Sync + 'static,
{
    async fn publish_swap_advertisement(
        &self,
        params: PublishSwapAdParams,
    ) -> Result<SwapOffer, ErrorObjectOwned> {
        let ckb_config = self.ckb_config.clone().ok_or_else(|| {
            ErrorObjectOwned::owned(-32603, "CKB config not available for signing", Option::<()>::None)
        })?;
        let chain_hash = self.ckb_chain_hash.unwrap_or_else(crate::fiber::network::get_chain_hash);
        if params.rate == 0 {
            return Err(ErrorObjectOwned::owned(-32602, "rate must be non-zero", Option::<()>::None));
        }
        let secret_key = ckb_config.read_secret_key().map_err(|e| {
            ErrorObjectOwned::owned(-32603, format!("failed to read secret key: {}", e), Option::<()>::None)
        })?;
        let privkey: fiber_types::Privkey = secret_key.into();
        let now = crate::now_timestamp_as_millis_u64();
        let expiry = now.checked_add(params.expiry_delta_millis).ok_or_else(|| {
            ErrorObjectOwned::owned(-32602, "expiry overflow", Option::<()>::None)
        })?;
        let base_packed = params.base_udt_script.map(|s| json_script_to_packed(s));
        let quote_packed = params.quote_udt_script.map(|s| json_script_to_packed(s));
        let ad = SwapAdvertisement::new_signed(
            chain_hash,
            base_packed,
            quote_packed,
            params.rate,
            params.fee_rate,
            &privkey,
            now,
            expiry,
        );
        if !ad.verify() {
            return Err(ErrorObjectOwned::owned(-32603, "failed to verify own advertisement", Option::<()>::None));
        }
        self.store.save_swap_advertisement(ad.clone());
        // Push into gossip so connected peers receive it immediately via
        // BroadcastMessagesFilterResult, instead of waiting for their next
        // active GetBroadcastMessages poll. Fire-and-forget: a missing gossip
        // actor must never fail the publish (the local save above already
        // makes the ad discoverable via active sync).
        if let Some(ref network_actor) = self.network_actor {
            let _ = network_actor.send_message(NetworkActorMessage::Command(
                NetworkActorCommand::BroadcastMessages(vec![
                    BroadcastMessageWithTimestamp::SwapAdvertisement(ad.clone()),
                ]),
            ));
        }
        Ok(ad_to_offer(ad))
    }

    async fn list_swap_advertisements(
        &self,
        params: ListSwapAdsParams,
    ) -> Result<ListSwapAdsResult, ErrorObjectOwned> {
        let now = crate::now_timestamp_as_millis_u64();
        let index = SwapAdIndex::new(self.store.clone());
        let filter = if params.base_udt_script.is_none() && params.quote_udt_script.is_none() {
            None
        } else {
            Some(AssetPairFilter {
                base_udt_script: params.base_udt_script.map(|s| json_script_to_packed(s)),
                quote_udt_script: params.quote_udt_script.map(|s| json_script_to_packed(s)),
            })
        };
        let ads = index.list_offers(filter, now);
        let offers = ads.into_iter().map(ad_to_offer).collect();
        Ok(ListSwapAdsResult { offers })
    }

    async fn get_best_swap_rate(
        &self,
        params: GetBestSwapRateParams,
    ) -> Result<GetBestSwapRateResult, ErrorObjectOwned> {
        let now = crate::now_timestamp_as_millis_u64();
        let index = SwapAdIndex::new(self.store.clone());
        let maybe = index.best_rate(
            &params.base_udt_script.map(|s| json_script_to_packed(s)),
            &params.quote_udt_script.map(|s| json_script_to_packed(s)),
            now,
        );
        Ok(GetBestSwapRateResult { offer: maybe.map(ad_to_offer) })
    }
}
