# Flux Protocol

Flux is a decentralized borrowing protocol built on the Radix Ledger using Scrypto. It allows users to mint `fUSD`, a stablecoin soft-pegged to the US Dollar, by depositing various accepted crypto assets as collateral into Collateralized Debt Positions (CDPs).

## Overview

The core mechanism involves users locking collateral into a CDP and minting `fUSD` against it, provided the loan remains sufficiently overcollateralized according to the specific collateral's Minimum Collateral Ratio (MCR). Users can manage their CDPs by adding/removing collateral, borrowing more `fUSD`, or repaying their debt. The protocol includes mechanisms for liquidations (when CDPs become undercollateralized) and redemptions (allowing users to swap `fUSD` for underlying collateral at face value, helping maintain the peg) facilitated by Stability Pools.

## Key Tokens

- **fUSD (`fUSD`):** The fungible stablecoin minted by the protocol.
- **CDP NFT (`fusdLOAN`):** A non-fungible token representing ownership and state of a specific Collateralized Debt Position.
- **Privileged Borrower NFT (`fusdPRIV`):** An optional non-fungible token that can grant special privileges to linked CDPs, such as redemption opt-outs or liquidation notice periods.
- **Controller Badge (`fusdCTRL`):** A fungible token used internally for authorization between protocol components.

## Components (Blueprints)

The protocol is modular, consisting of several interacting components:

1.  **`flux_component` (`src/flux_component.rs`)**:

    - The core logic engine of the protocol.
    - Manages accepted collateral types, their parameters (MCR, price), and storage vaults.
    - Handles the creation, modification, and closing of CDPs (represented by NFTs).
    - Controls the minting and burning of `fUSD`.
    - Implements the core logic for liquidations and redemptions.
    - Manages protocol parameters and operational stops.
    - Handles interest rate calculations and CDP sorting based on collateral ratios.
    - Manages Privileged Borrower NFTs and their linkage to CDPs.

2.  **`proxy` (`src/proxy.rs`)**:

    - The main user-facing entry point for the protocol.
    - Routes user actions (like opening CDPs, borrowing, repaying) to the appropriate underlying components (`Flux`, `FlashLoans`, `StabilityPools`).
    - Interacts with an external price Oracle component to fetch collateral prices needed for various operations.
    - Manages controller badges to authorize calls between components.
    - Handles user-provided proofs (e.g., for CDP ownership).
    - Provides administrative functions for protocol management.
    - Manages the protocol's DApp Definition.

3.  **`flash_loans` (`src/flash_loans.rs`)**:

    - Provides flash loan functionality for `fUSD`.
    - Allows users (authorized via the `Proxy`) to borrow `fUSD` that must be repaid within the same transaction, plus a small interest fee.
    - Uses transient `LoanReceipt` NFTs to track active flash loans within a transaction.

4.  **`stability_pools` (`src/stability_pools.rs`)**:

    - Manages pools of `fUSD` deposited by users, specific to each collateral type.
    - Acts as the primary source of liquidity for liquidating undercollateralized CDPs. `fUSD` from the pool covers the debt, and the pool receives the liquidated collateral (often at a discount).
    - Facilitates `fUSD` redemptions by coordinating with the `Flux` component.
    - Distributes rewards (from liquidations, fees) among pool depositors.
    - Triggers periodic interest charging on CDPs via the `Flux` component.
    - Implements a "Panic Mode" using centralized stablecoins for liquidations when a pool's `fUSD` is depleted.
    - Manages reward distribution between stability pools, liquidity rewards, and the payout component.

5.  **`payout_component` (`src/payout_component.rs`)**:

    - Manages the distribution of accumulated `fUSD` rewards from the protocol.
    - Receives a portion of protocol fees and rewards from stability pools.
    - Allows users to claim rewards by providing a specified payment token.
    - Supports configurable payment requirements and burning mechanisms.
    - Provides administrative functions for managing reward distribution parameters.
    - Emits events for tracking reward claims and parameter updates.

6.  **`shared_structs` (`src/shared_structs.rs`)**:

    - Defines common data structures used across multiple components, such as `Cdp`, `PrivilegedBorrowerData`, `CdpStatus`, and `CollateralInfoReturn`.

7.  **`events` (`src/events.rs`)**:
    - Defines the event structs emitted by the components (e.g., `EventNewCdp`, `EventLiquidateCdp`) for off-ledger tracking and integration.

## Build & Test

This project uses the Scrypto toolchain.

1.  **Install Scrypto:** Follow the instructions at [docs.radixdlt.com](https://docs.radixdlt.com/).
2.  **Build:** Navigate to the project root directory (`flux`) and run:
    ```bash
    scrypto build
    ```
3.  **Test:** (Assuming tests are implemented)
    ```bash
    scrypto test
    ```

## Deployment

Deploying the Flux protocol involves:

1.  Deploying dependent components (Oracle, Payout Component).
2.  Building the package (`scrypto build`).
3.  Publishing the package to the ledger.
4.  Instantiating the `Proxy` component using the `Proxy::new` function, providing the necessary addresses (Oracle, Payout, Owner Badge, initial Stablecoin). This function will automatically instantiate the `Flux`, `FlashLoans`, and `StabilityPools` components and link them together.
5.  Configuring the protocol via the `Proxy`'s admin methods (e.g., adding collateral types using `Proxy::new_collateral`).

## Protocol Parameters

The protocol includes several configurable parameters that can be adjusted by the admin:

1. **Stability Pool Parameters:**

   - Default payout split ratio
   - Default liquidity rewards split ratio
   - Default stability pool split ratio
   - Pool buy price modifiers
   - Contribution fees (flat and percentage)
   - Lowest interest history length

2. **Payout Component Parameters:**

   - Required payment token and amount for claiming rewards
   - Burn mechanism configuration
   - Reward distribution settings

3. **General Protocol Parameters:**
   - Minimum Collateral Ratios (MCR) for each collateral type
   - Liquidation parameters
   - Interest rate settings
   - Redemption fees and limits

## Security Features

1. **Controller Badge System:**

   - Strict authorization controls between components
   - Multi-signature requirements for critical operations

2. **Panic Mode:**

   - Emergency liquidation mechanism using centralized stablecoins
   - Configurable notice periods for privileged borrowers

3. **Redemption Protection:**
   - Optional redemption opt-out for privileged borrowers
   - Dynamic redemption fees based on protocol usage

## Links

- Website: [https://flux.ilikeitstable.com](https://flux.ilikeitstable.com)
- Documentation: [https://docs.ilikeitstable.com](https://docs.ilikeitstable.com)
- DAO: [https://dao.ilikeitstable.com](https://dao.ilikeitstable.com)
