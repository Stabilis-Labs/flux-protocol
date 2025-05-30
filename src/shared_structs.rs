//! # Proxy and fUSD Blueprint shared structs
//! Structs used by both the Proxy and fUSD component

use scrypto::prelude::*;

/// Data struct of a loan receipt / CDP receipt, gained when opening a CDP / loan
#[derive(ScryptoSbor, NonFungibleData, Clone, Debug)]
pub struct Cdp {
    /// Image of the NFT
    #[mutable]
    pub key_image_url: Url,
    /// The resource address of the collateral used for this loan / CDP.
    pub collateral_address: ResourceAddress,
    /// The current amount of collateral deposited in this CDP.
    #[mutable]
    pub collateral_amount: Decimal,
    /// The amount of debt denominated in the pool's internal unit (before applying the debt multiplier).
    #[mutable]
    pub pool_debt: Decimal,
    /// The ratio of collateral amount to pool debt (collateral_amount / pool_debt). Used for sorting CDPs.
    #[mutable]
    pub collateral_fusd_ratio: Decimal,
    /// The selected annual interest rate for this CDP. A rate of -420 indicates a privileged, irredeemable loan.
    #[mutable]
    pub interest: Decimal,
    /// Timestamp of the last time the interest rate for this CDP was changed.
    #[mutable]
    pub last_interest_change: Instant,
    /// The current status of the CDP / loan.
    #[mutable]
    pub status: CdpStatus,
    /// Optional local ID of the privileged borrower NFT linked to this CDP.
    #[mutable]
    pub privileged_borrower: Option<NonFungibleLocalId>,
}

/// Data struct for privileged borrower NFTs, granting special loan conditions.
#[derive(ScryptoSbor, NonFungibleData)]
pub struct PrivilegedBorrowerData {
    /// Image of the NFT
    #[mutable]
    pub key_image_url: Url,
    /// If true, loans linked to this borrower cannot be redeemed (but can still be liquidated).
    #[mutable]
    pub redemption_opt_out: bool,
    /// Optional liquidation notice period in minutes. If set, liquidations require this notice period. Amount in minutes.
    #[mutable]
    pub liquidation_notice: Option<i64>,
    /// The maximum number of CDPs that can be linked to this borrower NFT.
    #[mutable]
    pub max_coupled_loans: u64,
    /// A list of NonFungibleLocalIds of the CDPs currently linked to this borrower NFT.
    #[mutable]
    pub coupled_loans: Vec<NonFungibleLocalId>,
}

/// Represents the possible states of a Collateralized Debt Position (CDP).
#[derive(ScryptoSbor, PartialEq, Clone, Debug)]
pub enum CdpStatus {
    /// The CDP is active and meets its collateralization requirements.
    Healthy,
    /// The CDP has been liquidated due to insufficient collateral.
    Liquidated,
    /// The CDP has been closed through the redemption process.
    Redeemed,
    /// The CDP has been fully paid off and closed by the borrower.
    Closed,
    /// The CDP is undergoing a liquidation notice period (for privileged borrowers).
    Marked,
}

/// A struct providing a summarized view of a specific collateral's state within the Flux protocol.
/// This is often used for returning information via getter methods.
#[derive(ScryptoSbor, Clone)]
pub struct CollateralInfoReturn {
    /// The total amount of this collateral deposited across all CDPs using it.
    pub collateral_amount: Decimal,
    /// The total fUSD debt backed by this collateral across all CDPs.
    pub total_debt: Decimal,
    /// The resource address of the collateral token.
    pub resource_address: ResourceAddress,
    /// The Minimum Collateral Ratio required for this collateral.
    pub mcr: Decimal,
    /// The current USD price of the collateral according to the oracle.
    pub usd_price: Decimal,
    /// The amount of this collateral held in the main vault (backing active CDPs).
    pub vault: Decimal,
    /// The amount of this collateral held in the leftovers vault (from liquidations/redemptions).
    pub leftovers: Decimal,
    /// The amount of fUSD interest accrued but not yet charged/distributed for this collateral.
    pub uncharged_interest: Decimal,
    /// Indicates if this collateral type is currently accepted for opening new CDPs.
    pub accepted: bool,
}
