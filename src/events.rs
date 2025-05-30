//! Defines events emitted by the Ersatz protocol components.

use scrypto::prelude::*;
use crate::shared_structs::*;

/// Event emitted when a new collateral type is added to the Ersatz protocol.
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventAddCollateral {
    /// The `ResourceAddress` of the newly accepted collateral token.
    pub address: ResourceAddress,
    /// The Minimum Collateral Ratio (MCR) set for this collateral type.
    pub mcr: Decimal,
    /// The initial USD price set for this collateral type.
    pub usd_price: Decimal,
}

/// Event emitted when collateral is added to a liquidity pool (potentially outdated or specific usage).
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventAddPoolCollateral {
    /// The `ResourceAddress` of the collateral token added to the pool.
    pub address: ResourceAddress,
    /// The `ResourceAddress` identifying the specific pool or parent entity.
    pub parent_address: ResourceAddress,
}

/// Event emitted when a new Collateralized Debt Position (CDP) is opened.
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventNewCdp {
    /// The data associated with the newly created CDP.
    pub cdp: Cdp,
    /// The unique `NonFungibleLocalId` identifying the new CDP NFT.
    pub cdp_id: NonFungibleLocalId,
}

/// Event emitted when an existing Collateralized Debt Position (CDP) is updated.
/// This could happen due to borrowing more, repaying partially, adding/removing collateral, etc.
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventUpdateCdp {
    /// The updated data of the CDP.
    pub cdp: Cdp,
    /// The `NonFungibleLocalId` identifying the updated CDP NFT.
    pub cdp_id: NonFungibleLocalId,
}

#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventRedeemCdp {
    /// The updated data of the CDP.
    pub cdp: Cdp,
    /// The `NonFungibleLocalId` identifying the updated CDP NFT.
    pub cdp_id: NonFungibleLocalId,
    /// Fully redeemed or not
    pub fully_redeemed: bool,
}

#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventMarkCdp {
    /// The `NonFungibleLocalId` identifying the marked CDP NFT.
    pub cdp_id: NonFungibleLocalId,
}

#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventChargeInterest {
    /// The `ResourceAddress` of the collateral token.
    pub collateral_address: ResourceAddress,
    /// The start time of the interest charge.
    pub start: Option<Decimal>,
    /// The end time of the interest charge.
    pub end: Option<Decimal>,
    /// The interest for irredeemables.
    pub interest_for_irredeemables: Decimal,
    /// The total interest minted.
    pub fusd_minted: Decimal,
    /// The total interest charged (this includes previously uncharged interest)
    pub total_charged: Decimal,
}


/// Event emitted when a Collateralized Debt Position (CDP) is closed (fully repaid by borrower).
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventCloseCdp {
    /// The `NonFungibleLocalId` identifying the closed CDP NFT.
    pub cdp_id: NonFungibleLocalId,
}

/// Event emitted when a Collateralized Debt Position (CDP) is liquidated.
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventLiquidateCdp {
    /// The `NonFungibleLocalId` identifying the liquidated CDP NFT.
    pub cdp_id: NonFungibleLocalId,
}

/// Event emitted when parameters of an existing collateral type are changed.
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventChangeCollateral {
    /// The `ResourceAddress` of the collateral type being modified.
    pub address: ResourceAddress,
    /// The new Minimum Collateral Ratio (MCR), if changed.
    pub new_mcr: Option<Decimal>,
    /// The new USD price, if changed.
    pub new_usd_price: Option<Decimal>,
}

/// Event emitted when the internal peg or price calculation mechanism changes (potentially outdated or specific usage).
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct EventChangePeg {
    /// The new internal price or peg value.
    pub internal_price: Decimal,
}

/// Event emitted when a user contributes to a stability pool
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct StabilityPoolContributionEvent {
    /// The resource address of the collateral type for this pool
    pub collateral: ResourceAddress,
    /// The amount of fUSD contributed
    pub contribution_amount: Decimal,
    /// The amount of pool tokens received
    pub pool_tokens_received: Decimal,
}

/// Event emitted when a user withdraws from a stability pool
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct StabilityPoolWithdrawalEvent {
    /// The resource address of the collateral type for this pool
    pub collateral: ResourceAddress,
    /// The amount of pool tokens burned
    pub pool_tokens_burned: Decimal,
    /// The amount of fUSD received
    pub fusd_received: Decimal,
    /// The amount of collateral received
    pub collateral_received: Decimal,
}

/// Event emitted when collateral is bought directly from a stability pool
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct StabilityPoolBuyEvent {
    /// The resource address of the collateral type being bought
    pub collateral: ResourceAddress,
    /// The amount of fUSD paid
    pub fusd_paid: Decimal,
    /// The amount of collateral received
    pub collateral_received: Decimal,
    /// The effective price per unit of collateral
    pub effective_price: Decimal,
}

/// Event emitted when panic mode is activated for a CDP
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct PanicModeChangeEvent {
    /// The CDP ID that triggered panic mode
    pub cdp_id: NonFungibleLocalId,
    /// The timestamp when panic mode was activated
    pub activation_time: Instant,
    /// The change that occurred
    pub change: PanicModeEvent,
}

/// Event emitted when a panic mode liquidation occurs
#[derive(ScryptoSbor, ScryptoEvent, Clone)]
pub struct PanicModeLiquidationEvent {
    /// The CDP ID being liquidated
    pub cdp_id: NonFungibleLocalId,
    /// The amount of stablecoin paid
    pub stablecoin_paid: Decimal,
    /// The amount of collateral received
    pub collateral_received: Decimal,
}

/// Event emitted when rewards are claimed from the payout component
#[derive(ScryptoSbor, ScryptoEvent)]
pub struct PayoutClaimEvent {
    pub amount: Decimal,
}

/// Event emitted when the payout component fetches rewards from stability pools
#[derive(ScryptoSbor, ScryptoEvent)]
pub struct PayoutFetchRewardsEvent {
    pub amount: Decimal,
}

/// Event emitted when the required payment amount for claiming rewards is updated
#[derive(ScryptoSbor, ScryptoEvent)]
pub struct PayoutRequirementUpdateEvent {
    pub new_requirement: Decimal,
    pub burn: bool,
}

#[derive(ScryptoSbor, PartialEq, Clone)]
pub enum PanicModeEvent {
    Initiation,
    Activation,
    TooLateActivation,
}