//! # Flux Protocol Crate
//!
//! This crate contains the core Scrypto blueprints for the Flux protocol, a decentralized borrowing protocol
//! that allows users to mint fUSD stablecoins by depositing collateral into Collateralized Debt Positions (CDPs).
//!
//! The primary goal is to provide a stablecoin pegged to the US Dollar, generated in a decentralized manner.
//!
//! ## Modules
//!
//! The crate is organized into the following modules:
//!
//! - `flux_component`: Defines the main `Flux` component, which manages collateral types, CDPs, fUSD minting/burning,
//!   liquidations, redemptions, and core protocol parameters. This is the heart of the protocol's logic.
//! - `events`: Defines the various events emitted by the protocol components, allowing off-ledger services to track state changes.
//! - `flash_loans`: Implements a component for providing flash loans of fUSD, allowing users to borrow and repay fUSD
//!   within a single transaction.
//! - `proxy`: Defines a `Proxy` component that acts as the main entry point for user interactions. It routes calls
//!   to the `Flux`, `FlashLoans`, and `StabilityPools` components, handles oracle interactions for price updates,
//!   and manages authorization.
//! - `shared_structs`: Contains data structures shared across multiple components, such as `Cdp`, `PrivilegedBorrowerData`,
//!   and `CollateralInfoReturn`, promoting code reuse and consistency.
//! - `stability_pools`: Implements the `StabilityPools` component, which manages pools of fUSD contributed by users.
//!   These pools act as the first line of defense in absorbing debt during liquidations, enhancing protocol stability
//!   and providing yield opportunities for contributors. It also handles panic mode liquidations using centralized stablecoins.
//! - `payout_component`: Implements the payout component for the protocol.

pub mod flux_component;
pub mod events;
pub mod flash_loans;
pub mod proxy;
pub mod shared_structs;
pub mod stability_pools;
pub mod payout_component;
