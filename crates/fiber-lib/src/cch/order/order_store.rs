use crate::cch::error::CchStoreError;
use fiber_types::{CchOrder, Hash256, SwapOrder};

pub trait CchOrderStore {
    /// Gets an order from the store.
    ///
    /// Returns the order if found, a NotFound error otherwise.
    fn get_cch_order(&self, payment_hash: &Hash256) -> Result<CchOrder, CchStoreError>;

    /// Inserts a new order.
    ///
    /// Returns an error if an order with the same payment hash already exist in the database.
    fn insert_cch_order(&self, order: CchOrder) -> Result<(), CchStoreError>;

    /// Inserts or updates an order.
    fn update_cch_order(&self, order: CchOrder);

    /// Get an iterator to list keys of all orders.
    fn get_cch_order_keys_iter(&self) -> impl IntoIterator<Item = Hash256>;

    /// Deletes an order from the store.
    fn delete_cch_order(&self, payment_hash: &Hash256);
}

/// Store for Fiber-to-Fiber [`SwapOrder`]s.
///
/// Mirrors [`CchOrderStore`] under a separate key prefix so the two order
/// kinds never collide, even for the same payment hash.
pub trait SwapOrderStore {
    /// Gets a swap order from the store.
    fn get_swap_order(&self, payment_hash: &Hash256) -> Result<SwapOrder, CchStoreError>;

    /// Inserts a new swap order; errors on duplicate payment hash.
    fn insert_swap_order(&self, order: SwapOrder) -> Result<(), CchStoreError>;

    /// Inserts or updates a swap order.
    fn update_swap_order(&self, order: SwapOrder);

    /// Iterate all swap order keys.
    fn get_swap_order_keys_iter(&self) -> impl IntoIterator<Item = Hash256>;

    /// Deletes a swap order from the store.
    fn delete_swap_order(&self, payment_hash: &Hash256);
}
