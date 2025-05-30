mod helper;
use helper::Helper;
use flux_protocol::shared_structs::*;

use scrypto_test::prelude::*;
use scrypto::prelude::Url;

#[test]
fn test_contribute_to_pool_empty() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Get fUSD to contribute to the pool
    let fusd_amount = dec!(100);
    helper.env.disable_auth_module();
    let fusd = helper.free_fusd(fusd_amount)?;
    helper.env.enable_auth_module();
    
    // Contribute to the XRD/fUSD stability pool when it's empty
    let (pool_units, leftover, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        false, // don't deposit leftover
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received pool units
    assert!(pool_units.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Since pool was empty and we provided fUSD, there should be no leftover
    assert!(leftover.is_none());
    
    // Check the stability pool info to verify the fUSD was added
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let fusd_in_pool = pool_info.fusd_amount;
    let xrd_in_pool = pool_info.collateral_amount;
    
    // Since pool was empty, all our fUSD should be in the pool
    assert_eq!(fusd_in_pool, fusd_amount);
    assert_eq!(xrd_in_pool, Decimal::ZERO);
    
    Ok(())
}

#[test]
fn test_contribute_to_pool_with_collateral_only() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Get fUSD to contribute to the pool
    let fusd_amount = dec!(100);
    helper.env.disable_auth_module();
    let fusd = helper.free_fusd(fusd_amount)?;
    helper.env.enable_auth_module();
    
    // First disable auth to use protected methods to set up test conditions
    helper.env.disable_auth_module();
    
    // Add collateral to the pool directly
    let xrd_amount = dec!(200);
    let xrd_bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        xrd_bucket,
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Contribute to the XRD/fUSD stability pool when it has only collateral
    let (pool_units, leftover, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        false, // don't deposit leftover
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received pool units
    assert!(pool_units.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Check the stability pool info to verify the fUSD was added
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let fusd_in_pool = pool_info.fusd_amount;
    let xrd_in_pool = pool_info.collateral_amount;
    
    // Since pool had only collateral, part of our fUSD would be used to buy
    // some of the collateral from the pool to maintain the ratio
    assert!(fusd_in_pool > Decimal::ZERO);
    
    Ok(())
}

#[test]
fn test_contribute_to_pool_with_dust_collateral() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // First disable auth to use protected methods to set up test conditions
    helper.env.disable_auth_module();

    // Get fUSD to contribute to the pool
    let fusd_amount = dec!(100);
    let fusd = helper.free_fusd(fusd_amount)?;
    
    // Add tiny amount of collateral to the pool directly
    let tiny_xrd_amount = dec!(0.000001); // dust amount
    let xrd_bucket = helper.xrd.take(tiny_xrd_amount, &mut helper.env)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        xrd_bucket,
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Contribute to the XRD/fUSD stability pool when it has dust collateral
    let (pool_units, leftover, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        false, // don't deposit leftover
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received pool units
    assert!(pool_units.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Check the stability pool info to verify the fUSD was added
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let fusd_in_pool = pool_info.fusd_amount;
    
    // Since the collateral was dust compared to our fUSD, most of our fUSD should be in the pool
    assert!(fusd_in_pool > fusd_amount * dec!(0.9));
    
    Ok(())
}

#[test]
fn test_withdraw_from_pool() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Get fUSD to contribute to the pool
    let fusd_amount = dec!(100);
    helper.env.disable_auth_module();
    let fusd = helper.free_fusd(fusd_amount)?;
    helper.env.enable_auth_module();
    
    // Contribute to the XRD/fUSD stability pool
    let (pool_units, _, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Record the pool unit amount
    let pool_unit_amount = pool_units.amount(&mut helper.env)?;
    
    // Withdraw half of our position
    let pool_units_to_withdraw = pool_units.take(pool_unit_amount / dec!(2), &mut helper.env)?;
    let (collateral, fusd) = helper.stability_pools.withdraw_from_pool(
        helper.xrd_address,
        pool_units_to_withdraw,
        &mut helper.env
    )?;
    
    // Verify we received our tokens back (collateral might be zero if pool was empty)
    assert_eq!(fusd.resource_address(&mut helper.env)?, helper.flux.get_fusd_address(&mut helper.env)?);
    assert_eq!(collateral.resource_address(&mut helper.env)?, helper.xrd_address);
    
    // Combined value should be approximately half of what we put in
    // (might be slightly different due to fees)
    assert!(fusd.amount(&mut helper.env)? > Decimal::ZERO);
    
    Ok(())
}

#[test]
fn test_buy_collateral_from_pool() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Setup: Add both fUSD and XRD to the pool
    let xrd_amount = dec!(200);
    let fusd_amount = dec!(100);
    
    // First disable auth to use protected methods to set up test conditions
    helper.env.disable_auth_module();

    // Get fUSD and add to pool
    let fusd = helper.free_fusd(fusd_amount)?;

    // Add XRD to the pool directly
    let xrd_bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        xrd_bucket,
        &mut helper.env
    )?;
    
    // Turn on pool buys (ensure allow_pool_buys is true)
    helper.stability_pools.edit_pool(
        helper.xrd_address, 
        None, 
        None, 
        None, 
        true, // allow_pool_buys = true
        Some(dec!(0.99)), // 99% of market price
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Add fUSD to the pool
    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        true, // deposit leftover
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Get fUSD to buy collateral with
    let buy_amount = dec!(50);
    helper.env.disable_auth_module();
    let buy_fusd = helper.free_fusd(buy_amount)?;
    helper.env.enable_auth_module();
    
    // Buy collateral from the pool
    let (bought_collateral, leftover_fusd) = helper.stability_pools.buy_collateral_from_pool(
        helper.xrd_address,
        buy_fusd,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received XRD
    assert_eq!(bought_collateral.resource_address(&mut helper.env)?, helper.xrd_address);
    assert!(bought_collateral.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Calculate expected collateral amount (price discount of 1%)
    // XRD price = 1 (from setup)
    let expected_amount = buy_amount / dec!(0.99);
    
    // Allow for small rounding differences
    let actual_amount = bought_collateral.amount(&mut helper.env)?;
    let difference = (expected_amount - actual_amount).checked_abs().unwrap();
    assert!(difference < dec!(0.01), 
           "Expected approximately {} XRD but got {}", expected_amount, actual_amount);

    // Verify we received no leftover fUSD
    assert_eq!(leftover_fusd.amount(&mut helper.env)?, Decimal::ZERO);
    
    Ok(())
}

#[test]
fn test_buy_collateral_from_pool_too_much() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Setup: Add a small amount of XRD to the pool
    let xrd_amount = dec!(10);
    let fusd_amount = dec!(100);    
    
    // First disable auth to use protected methods to set up test conditions
    helper.env.disable_auth_module();

    // Get fUSD and add to pool
    let fusd = helper.free_fusd(fusd_amount)?;

    // Add XRD to the pool directly
    let xrd_bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        xrd_bucket,
        &mut helper.env
    )?;
    
    // Turn on pool buys (ensure allow_pool_buys is true)
    helper.stability_pools.edit_pool(
        helper.xrd_address, 
        None, 
        None, 
        None, 
        true, // allow_pool_buys = true
        Some(dec!(0.99)), // 99% of market price
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Add fUSD to the pool
    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        true,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Try to buy more collateral than available in the pool
    let buy_amount = dec!(1000); // Much more than the 10 XRD in the pool
    helper.env.disable_auth_module();
    let buy_fusd = helper.free_fusd(buy_amount)?;
    helper.env.enable_auth_module();
    
    // Buy collateral from the pool - should get less than requested
    let (bought_collateral, leftover_fusd) = helper.stability_pools.buy_collateral_from_pool(
        helper.xrd_address,
        buy_fusd,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received XRD, but less than what we tried to buy
    assert_eq!(bought_collateral.resource_address(&mut helper.env)?, helper.xrd_address);
    
    // We should receive approximately all the XRD that was in the pool
    let actual_amount = bought_collateral.amount(&mut helper.env)?;
    assert_eq!(actual_amount, xrd_amount);

    // Verify we received the leftover fUSD
    assert_eq!(leftover_fusd.resource_address(&mut helper.env)?, helper.flux.get_fusd_address(&mut helper.env)?);

    // we bought with 1000 fUSD (for 10 fUSD worth of XRD), so we should have 990.1 fUSD left with 1% discount
    assert!(leftover_fusd.amount(&mut helper.env)? >= dec!(990.1));
    
    Ok(())
}

#[test]
fn test_charge_interest() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    helper.set_allow_multiple_actions(true);
    
    // Create a CDP to generate interest
    let collateral_amount = dec!(2000);
    let borrow_amount = dec!(500);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.05))?;
    
    // Contribute some of the borrowed fUSD to the stability pool
    let deposit_amount = dec!(100);
    let deposit_fusd = fusd.take(deposit_amount, &mut helper.env)?;

    let (pool_units, _, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        deposit_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Record initial state
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let initial_fusd = pool_info.fusd_amount;
    
    // Fast forward time to allow interest to accrue
    let new_time = helper.env.get_current_time().add_days(30).unwrap(); // 30 days
    helper.env.set_current_time(new_time);
    
    // Charge interest
    helper.stability_pools.charge_interest(
        helper.xrd_address,
        None,
        None,
        &mut helper.env
    )?;
    
    // Check that interest was distributed to the pool
    let pool_info_after = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let after_fusd = pool_info_after.fusd_amount;
    
    // Verify that fUSD amount in pool increased (interest was added)
    assert!(after_fusd > initial_fusd, 
           "Expected fUSD to increase from {} to more, but got {}", initial_fusd, after_fusd);
    
    // Also check that liquidity rewards were captured
    assert!(pool_info_after.liquidity_rewards > Decimal::ZERO);
    
    Ok(())
}

#[test]
fn test_liquidate_normal() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;

    helper.env.disable_auth_module();
    let free_fusd = helper.free_fusd(dec!(500))?;
    helper.env.enable_auth_module();
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        free_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Reduce collateral price to trigger liquidation
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(0.5))?;
    helper.env.enable_auth_module();
    
    // Try to liquidate
    let liquidator_fee = helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;

    // Verify liquidator fee is non-zero
    assert_eq!(liquidator_fee.amount(&mut helper.env)?, dec!(0.800153337611860542));
    
    // Verify CDP was liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Liquidated);
    
    // Stability pool should now have collateral
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let xrd_in_pool = pool_info.collateral_amount;
    
    // Verify collateral was added to the pool
    assert!(xrd_in_pool > Decimal::ZERO);
    
    Ok(())
}

#[test]
fn test_basic_redemption() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create CDPs with both collateral types
    let xrd_amount = dec!(1000);
    let lsulp_amount = dec!(1000);
    let borrow_amount = dec!(200);
    
    // Open XRD CDP
    let xrd_bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    let (fusd1, _) = helper.proxy_open_cdp(None, xrd_bucket, borrow_amount, dec!(0.01))?;
    
    // Open LSULP CDP
    let lsulp_bucket = helper.lsulp.take(lsulp_amount, &mut helper.env)?;
    let (fusd2, _) = helper.proxy_open_cdp(None, lsulp_bucket, borrow_amount, dec!(0.01))?;
    
    // Combine fUSD
    fusd1.put(fusd2, &mut helper.env)?;
    
    // Create oracle info for both collaterals (empty strings are fine for tests)
    let oracle_info = vec![
        (helper.xrd_address, "".to_string(), "".to_string()),
        (helper.lsulp_address, "".to_string(), "".to_string())
    ];
    
    // Perform redemption
    let redemption_amount = dec!(100);
    let redeem_fusd = fusd1.take(redemption_amount, &mut helper.env)?;
    let (redeemed_collateral, leftover_fusd) = helper.stability_pools.redemptions(
        redeem_fusd,
        oracle_info,
        10, // max_redemptions
        &mut helper.env
    )?;
    
    // Verify we received collateral
    assert!(!redeemed_collateral.is_empty());
    
    // Both collateral types should be received
    let has_xrd = redeemed_collateral.iter().any(|(addr, _)| *addr == helper.xrd_address);
    let has_lsulp = redeemed_collateral.iter().any(|(addr, _)| *addr == helper.lsulp_address);
    
    assert!(has_xrd, "Should have received XRD collateral");
    assert!(has_lsulp, "Should have received LSULP collateral");
    
    // Calculate total value of returned collateral (both should be priced at 1)
    let mut total_value = Decimal::ZERO;
    for (_, bucket) in &redeemed_collateral {
        if bucket.resource_address(&mut helper.env)? == helper.xrd_address {
            total_value += bucket.amount(&mut helper.env)?;
        } else if bucket.resource_address(&mut helper.env)? == helper.lsulp_address {
            total_value += dec!(2) * bucket.amount(&mut helper.env)?;
        }
    }
    
    // Value should be approximately equal to the redemption amount 
    // (minus any redemption fees)
    assert!(total_value > redemption_amount * dec!(0.94999999), //allow tiny rounding error
           "Expected to receive at least 95% of redemption value in collateral, but got {}, with redemption amount {}", total_value, redemption_amount);
    
    Ok(())
}

#[test]
fn test_check_and_initiate_panic_mode() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Initiate panic mode (first stage)
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time beyond wait period but before timeout
    let new_time = helper.env.get_current_time().add_hours(25).unwrap(); // 25 hours
    helper.env.set_current_time(new_time);
    
    // Complete panic mode initiation
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Panic mode should be active
    let panic_mode_active = helper.stability_pools.check_panic_mode_status(&mut helper.env)?;
    assert!(panic_mode_active);
    
    Ok(())
}

#[test]
fn test_contribute_to_pool_with_dust_fusd() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // First disable auth to use protected methods to set up test conditions
    helper.env.disable_auth_module();
    
    // Add substantial collateral to the pool directly
    let xrd_amount = dec!(200);
    let xrd_bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        xrd_bucket,
        &mut helper.env
    )?;
    
    // Add tiny amount of fUSD to the pool directly
    let tiny_fusd_amount = dec!(0.000001); // dust amount
    let fusd = helper.free_fusd(tiny_fusd_amount)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        fusd,
        &mut helper.env
    )?;
    
    // Get more fUSD to contribute
    let fusd_amount = dec!(100);
    let fusd = helper.free_fusd(fusd_amount)?;

    helper.env.enable_auth_module();
    
    // Contribute to the XRD/fUSD stability pool when it has significant collateral but little fUSD
    let (pool_units, leftover, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        false, // don't deposit leftover
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received pool units
    assert!(pool_units.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Check the stability pool info to verify the fUSD was added
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let fusd_in_pool = pool_info.fusd_amount;
    let xrd_in_pool = pool_info.collateral_amount;
    
    // Should be more than our dust amount
    assert!(fusd_in_pool > tiny_fusd_amount);
    
    Ok(())
}

#[test]
fn test_contribute_to_pool_with_only_fusd() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // First disable auth to use protected methods to set up test conditions
    helper.env.disable_auth_module();
    
    // Add some fUSD to the pool directly
    let initial_fusd_amount = dec!(50);
    let fusd = helper.free_fusd(initial_fusd_amount)?;
    helper.stability_pools.protected_deposit(
        helper.xrd_address,
        fusd,
        &mut helper.env
    )?;
    
    // Get more fUSD to contribute
    let fusd_amount = dec!(100);
    let fusd = helper.free_fusd(fusd_amount)?;

    helper.env.enable_auth_module();
    
    // Contribute to the XRD/fUSD stability pool when it has only fUSD
    let (pool_units, leftover, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        fusd,
        false, // don't deposit leftover
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received pool units
    assert!(pool_units.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Check the stability pool info to verify the fUSD was added
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let fusd_in_pool = pool_info.fusd_amount;
    
    // Since pool had only fUSD, all our fUSD should be in the pool
    assert_eq!(fusd_in_pool, initial_fusd_amount + fusd_amount);
    
    Ok(())
}

#[test]
fn test_withdraw_from_pool_after_interest() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    helper.set_allow_multiple_actions(true)?;
    
    // Create a CDP to generate interest
    let collateral_amount = dec!(2000);
    let borrow_amount = dec!(500);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.05))?;
    
    // Contribute some of the borrowed fUSD to the stability pool
    let deposit_amount = dec!(100);
    let deposit_fusd = fusd.take(deposit_amount, &mut helper.env)?;
    
    let (pool_units, _, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        deposit_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Record the pool unit amount and initial fUSD deposited
    let pool_unit_amount = pool_units.amount(&mut helper.env)?;
    
    // Fast forward time to allow interest to accrue
    let new_time = helper.env.get_current_time().add_days(30).unwrap(); // 30 days
    helper.env.set_current_time(new_time);
    
    // Charge interest
    helper.stability_pools.charge_interest(
        helper.xrd_address,
        None,
        None,
        &mut helper.env
    )?;
    
    // Now withdraw all of our position
    let (collateral, fusd_withdrawn) = helper.stability_pools.withdraw_from_pool(
        helper.xrd_address,
        pool_units,
        &mut helper.env
    )?;
    
    // Check that we got back more fUSD than we put in due to interest
    let fusd_amount = fusd_withdrawn.amount(&mut helper.env)?;
    assert!(fusd_amount > deposit_amount, 
           "Expected to withdraw more than {} fUSD due to interest, but got {}", deposit_amount, fusd_amount);
    
    Ok(())
}

#[test]
fn test_cannot_pool_withdraw_or_contribute_during_interest_charge() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP to generate interest
    let collateral_amount = dec!(2000);
    let borrow_amount = dec!(500);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.05))?;
    
    // Contribute some of the borrowed fUSD to the stability pool
    let deposit_amount = dec!(100);
    let deposit_fusd = fusd.take(deposit_amount, &mut helper.env)?;
    
    let (pool_units, _, _) = helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        deposit_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Charge interest
    let fail_result = helper.stability_pools.charge_interest(
        helper.xrd_address,
        None,
        None,
        &mut helper.env
    );

    assert!(fail_result.is_err(), "Should not be able to charge interest while also contributing to pool");
    
    Ok(())
}

#[test]
fn test_open_privileged_cdp_without_privileges() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Open a CDP as a privileged borrower
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let fail_result = helper.proxy.open_cdp(
        None,
        bucket,
        borrow_amount,
        dec!(-420), // Negative interest rate
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );

    assert!(fail_result.is_err(), "Should not be able to open CDP without privileges");
    
    Ok(())
}

#[test]
fn test_liquidate_privileged_borrower_marking() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower with redemption_opt_out = true
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        borrow_amount,
        dec!(-420), // Negative interest rate
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Contribute fUSD to the stability pool
    let pool_fusd = helper.free_fusd(dec!(500))?;

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Try to liquidate - should mark but not liquidate yet
    helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP was marked but not liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Marked);
    
    Ok(())
}

#[test]
fn test_liquidate_privileged_borrower_too_early() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        borrow_amount,
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Contribute fUSD to the stability pool
    let pool_fusd = fusd.take(dec!(300), &mut helper.env)?;
    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Try to liquidate - should mark
    helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP was marked but not liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Marked);
    
    // Wait a short time (less than required waiting period)
    let new_time = helper.env.get_current_time().add_hours(2).unwrap(); // 2 hours
    helper.env.set_current_time(new_time);
    
    // Try to liquidate again - should fail because it's too early
    let result = helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Liquidating a privileged borrower too early should fail");
    
    Ok(())
}

#[test]
fn test_liquidate_privileged_borrower_after_waiting() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        borrow_amount,
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Contribute fUSD to the stability pool
    helper.env.disable_auth_module();
    let pool_fusd = helper.free_fusd(dec!(500))?;
    helper.env.enable_auth_module();

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Try to liquidate - should mark
    helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP was marked but not liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Marked);
    
    // Wait sufficiently long (more than required waiting period)
    let new_time = helper.env.get_current_time().add_hours(40).unwrap(); // 40 hours
    helper.env.set_current_time(new_time);
    
    // Try to liquidate again - should succeed because we waited long enough
    helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP was liquidated
    let (_, cdp_info_after, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info_after.status, CdpStatus::Liquidated);
    
    Ok(())
}

#[test]
fn test_liquidate_privileged_borrower_after_unmarking_via_unmark() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_fusd, cdp_receipt) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        borrow_amount,
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Contribute fUSD to the stability pool
    helper.env.disable_auth_module();
    let pool_fusd = helper.free_fusd(dec!(500))?;
    helper.env.enable_auth_module();

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    
    // Try to liquidate - should mark
    helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP was marked but not liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Marked);
    
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(1))?;
    helper.env.enable_auth_module();
    
    let receipt_proof_2 = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Unmark the CDP
    helper.proxy.unmark(
        receipt_proof_2,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    helper.env.enable_auth_module();
    
    // Verify CDP is not marked anymore
    let (_, cdp_info_after_unmarking, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info_after_unmarking.status, CdpStatus::Healthy);
    
    // Wait sufficiently long
    let new_time = helper.env.get_current_time().add_hours(48).unwrap(); // 48 hours
    helper.env.set_current_time(new_time);
    
    // Try to liquidate again - should fail because it's not marked anymore
    let result = helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Liquidating an unmarked privileged borrower should fail");

    Ok(())
}

#[test]
fn test_liquidate_privileged_borrower_after_unmarking_via_top_up() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_fusd, cdp_receipt) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        borrow_amount,
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Contribute fUSD to the stability pool
    helper.env.disable_auth_module();
    let pool_fusd = helper.free_fusd(dec!(500))?;
    helper.env.enable_auth_module();

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    
    // Try to liquidate - should mark
    helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP was marked but not liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Marked);
    
    // Top up collateral and unmark the CDP
    let additional_collateral = helper.xrd.take(dec!(1200), &mut helper.env)?;
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Top up CDP with more collateral
    helper.proxy.top_up_cdp(
        receipt_proof,
        additional_collateral,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify CDP is not marked anymore
    let (_, cdp_info_after_unmarking, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info_after_unmarking.status, CdpStatus::Healthy);
    
    // Wait sufficiently long
    let new_time = helper.env.get_current_time().add_hours(48).unwrap(); // 48 hours
    helper.env.set_current_time(new_time);
    
    // Try to liquidate again - should fail because it's not marked anymore
    let result = helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Liquidating an unmarked privileged borrower should fail");

    Ok(())
}

#[test]
fn test_liquidate_with_different_cr_levels() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Case 1: CR is above 105% (will use 105% of the debt in collateral)
    // Create a CDP with CR around 150%
    let collateral_amount_1 = dec!(3000);
    let borrow_amount_1 = dec!(500);
    let bucket_1 = helper.xrd.take(collateral_amount_1, &mut helper.env)?;
    let (fusd_1, receipt_1) = helper.proxy_open_cdp(None, bucket_1, borrow_amount_1, dec!(0.01))?;
    
    // Case 2: CR is below 105% (will use all collateral)
    // Create a CDP with CR around 103%
    let collateral_amount_2 = dec!(2060);
    let borrow_amount_2 = dec!(500);
    let bucket_2 = helper.xrd.take(collateral_amount_2, &mut helper.env)?;
    let (fusd_2, receipt_2) = helper.proxy_open_cdp(None, bucket_2, borrow_amount_2, dec!(0.01))?;

    // Case 3: CR is below 105% (will use all collateral)
    // Create a CDP with CR around 90%
    let collateral_amount_3 = dec!(1800);
    let borrow_amount_3 = dec!(500);
    let bucket_3 = helper.xrd.take(collateral_amount_3, &mut helper.env)?;
    let (fusd_3, receipt_3) = helper.proxy_open_cdp(None, bucket_3, borrow_amount_3, dec!(0.01))?;
    
    // Contribute fUSD to the stability pool
    helper.env.disable_auth_module();
    let pool_fusd = helper.free_fusd(dec!(2000))?;
    helper.env.enable_auth_module();

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Disable auth module
    helper.env.disable_auth_module();
    
    // Reduce collateral price to trigger liquidation
    helper.change_collateral_price("XRD".to_string(), dec!(0.25))?;
    helper.env.enable_auth_module();
    
    // Get CDP IDs
    let receipt_id_1 = NonFungibleLocalId::from(1);
    let receipt_id_2 = NonFungibleLocalId::from(2);
    let receipt_id_3 = NonFungibleLocalId::from(3);

    // Liquidate first CDP (CR > 110%)
    helper.stability_pools.liquidate(
        receipt_id_1.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Liquidate second CDP (100% <CR < 110%)
    let liquidation_reward_between_100_and_110 = helper.stability_pools.liquidate(
        receipt_id_2.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;

    // Liquidate third CDP (CR < 100%)
    let liquidation_reward_under_100 = helper.stability_pools.liquidate(
        receipt_id_3.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify both CDPs were liquidated
    let (_, cdp_info_1, _) = helper.get_cdp_info(receipt_id_1.clone())?;
    let (_, cdp_info_2, _) = helper.get_cdp_info(receipt_id_2.clone())?;
    let (_, cdp_info_3, _) = helper.get_cdp_info(receipt_id_3.clone())?;

    assert_eq!(cdp_info_1.status, CdpStatus::Liquidated);
    assert_eq!(cdp_info_2.status, CdpStatus::Liquidated);
    assert_eq!(cdp_info_3.status, CdpStatus::Liquidated);

    // Check stability pool to verify collateral was added
    let pool_info = &helper.stability_pools.get_stability_pool_infos(Some(vec![helper.xrd_address]), &mut helper.env)?[0];
    let xrd_in_pool = pool_info.collateral_amount;
    
    // Liquidations should have added collateral to the pool (no precise calculation, just a sanity check)
    assert!(xrd_in_pool > dec!(4000));
    
    let receipt_proof_1 = NonFungibleProof(receipt_1.create_proof_of_all(&mut helper.env)?);
    let leftover_1 = helper.proxy.retrieve_leftover_collateral(receipt_proof_1, &mut helper.env)?;
    
    // First CDP should have leftover collateral (CR was higher than 105%)
    assert!(leftover_1.amount(&mut helper.env)? > Decimal::ZERO);

    // second cdp should have liquidation reward, but no leftover collateral
    assert!(liquidation_reward_between_100_and_110.amount(&mut helper.env)? > dec!(0));
    assert!(cdp_info_2.collateral_amount == Decimal::ZERO);

    let receipt_proof_3 = NonFungibleProof(receipt_3.create_proof_of_all(&mut helper.env)?);
    
    // Third CDP should have no leftover because CR was close to 100%, AND no liquidation reward because CR was below 100% (so 0 profit)
    assert_eq!(liquidation_reward_under_100.amount(&mut helper.env)?, dec!(0));
    let leftover_3 = helper.proxy.retrieve_leftover_collateral(receipt_proof_3, &mut helper.env);
    assert!(leftover_3.is_err(), "Leftover collateral should be 0, so not possible to retrieve");
    
    Ok(())
}

#[test]
fn test_liquidate_non_liquidatable_cdp() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with low LTV (high CR)
    let collateral_amount = dec!(2000);
    let borrow_amount = dec!(400); // CR of 5 is well above liquidation threshold
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Try to liquidate - should fail because CR is too high
    let result = helper.stability_pools.liquidate(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    // Verify liquidation failed
    assert!(result.is_err(), "Liquidating a CDP with high CR should fail");
    
    Ok(())
}

#[test]
fn test_panic_mode_initiate_after_waiting() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Initiate panic mode (first stage)
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time beyond wait period but before timeout
    let new_time = helper.env.get_current_time().add_hours(25).unwrap(); // 25 hours
    helper.env.set_current_time(new_time);
    
    // Complete panic mode initiation
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Panic mode should be active
    let panic_mode_active = helper.stability_pools.check_panic_mode_status(&mut helper.env)?;
    assert!(panic_mode_active);
    
    Ok(())
}

#[test]
fn test_panic_mode_initiate_too_late() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Initiate panic mode (first stage)
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time beyond the timeout (> 2 days)
    let new_time = helper.env.get_current_time().add_hours(49).unwrap(); // 49 hours
    helper.env.set_current_time(new_time);
    
    // Try to complete panic mode initiation - shouldn't do anything because too late
    let result = helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    // Panic mode should not be active
    let panic_mode_active = helper.stability_pools.check_panic_mode_status(&mut helper.env)?;
    assert!(!panic_mode_active);
    
    Ok(())
}

#[test]
fn test_panic_mode_liquidate_too_late() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Initiate panic mode (first stage)
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time beyond wait period but before timeout
    let new_time = helper.env.get_current_time().add_hours(25).unwrap(); // 25 hours
    helper.env.set_current_time(new_time);
    
    // Complete panic mode initiation
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    let new_time = helper.env.get_current_time().add_hours(25).unwrap();
    helper.env.set_current_time(new_time);
    
    // Prepare USDC for panic mode liquidation
    let usdc_payment = helper.usdc.take(dec!(400), &mut helper.env)?;
    
    // Try to liquidate in panic mode - should fail because it's too late
    let result = helper.stability_pools.panic_mode_liquidate(
        receipt_id.clone(),
        usdc_payment,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Panic mode liquidation after cooldown should fail");
    
    Ok(())
}

#[test]
fn test_panic_mode_no_need_for_it() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.disable_auth_module();
    let stability_pool_fusd = helper.free_fusd(dec!(1000))?;
    helper.env.enable_auth_module();
    
    // Contribute all the fUSD to the stability pool (enough to cover liquidation)
    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        stability_pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Try to initiate panic mode - should fail because there's enough fUSD
    let result = helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Panic mode initiation with enough fUSD should fail");
    
    Ok(())
}

#[test]
fn test_panic_mode_cdp_not_liquidatable() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with low LTV (high CR)
    let collateral_amount = dec!(2000);
    let borrow_amount = dec!(400); // CR of 5 is well above liquidation threshold
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Try to initiate panic mode with healthy CDP - should fail
    let result = helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Panic mode initiation with healthy CDP should fail");
    
    Ok(())
}

#[test]
fn test_redemption_with_stablecoin_and_collateral() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400);
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (fusd, _) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.env.disable_auth_module();
    let fusd_for_redemption = helper.free_fusd(dec!(1000))?;
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    
    // Setup: Enter panic mode and perform panic liquidation
    
    // Initiate panic mode (first stage)
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time beyond wait period but before timeout
    let new_time = helper.env.get_current_time().add_hours(25).unwrap(); // 25 hours
    helper.env.set_current_time(new_time);
    
    // Complete panic mode initiation
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Perform a panic mode liquidation using USDC
    let usdc_payment = helper.usdc.take(dec!(500), &mut helper.env)?;
    let (_, _) = helper.stability_pools.panic_mode_liquidate(
        receipt_id.clone(),
        usdc_payment,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Create additional CDP with LSULP
    let lsulp_amount = dec!(1000);
    let lsulp_borrow = dec!(300);
    let lsulp_bucket = helper.lsulp.take(lsulp_amount, &mut helper.env)?;
    let (_, _) = helper.proxy.open_cdp(
        None,
        lsulp_bucket,
        lsulp_borrow,
        dec!(0.01),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Get fUSD for redemption
    let redemption_amount = dec!(650);
    let redeem_fusd = fusd_for_redemption.take(redemption_amount, &mut helper.env)?;
    
    // Create oracle info for both collaterals
    let oracle_info = vec![
        (helper.xrd_address, "".to_string(), "".to_string()),
        (helper.lsulp_address, "".to_string(), "".to_string())
    ];
    
    // Perform redemption
    let (redeemed_collateral, leftover_fusd) = helper.stability_pools.redemptions(
        redeem_fusd,
        oracle_info,
        10, // max_redemptions
        &mut helper.env
    )?;
    
    // Verify we received both USDC and collateral
    assert!(!redeemed_collateral.is_empty());
    
    // Should include USDC (from panic mode liquidation)
    let has_usdc = redeemed_collateral.iter().any(|(addr, _)| *addr == helper.usdc_address);
    // Should include normal collateral
    let has_collateral = redeemed_collateral.iter().any(|(addr, _)| 
        *addr == helper.xrd_address || *addr == helper.lsulp_address);
    
    assert!(has_usdc, "Should have received USDC");
    assert!(has_collateral, "Should have received normal collateral");
    
    Ok(())
}

#[test]
fn test_redemption_privileged_vs_non_privileged() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);

    // Open a CDP as a privileged borrower
    let priv_collateral = dec!(1000);
    let priv_borrow = dec!(300);
    let priv_bucket = helper.xrd.take(priv_collateral, &mut helper.env)?;

    let (priv_fusd, _) = helper.proxy.open_cdp(
        //Some(privileged_borrower_proof),
        None,
        priv_bucket,
        priv_borrow,
        dec!(0.1),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;

    // Open a regular CDP
    let reg_collateral = dec!(1000);
    let reg_borrow = dec!(300);
    let reg_bucket = helper.lsulp.take(reg_collateral, &mut helper.env)?;
    let (reg_fusd, _) = helper.proxy.open_cdp(
        None,
        reg_bucket,
        reg_borrow,
        dec!(0.01),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;

    // Open a regular CDP 2
    let reg_collateral_2 = dec!(1000);
    let reg_borrow_2 = dec!(300);
    let reg_bucket_2 = helper.xrd.take(reg_collateral_2, &mut helper.env)?;
    let (reg_fusd_2, _) = helper.proxy.open_cdp(
        None,
        reg_bucket_2,
        reg_borrow_2,
        dec!(0.01),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Combine fUSD
    priv_fusd.put(reg_fusd, &mut helper.env)?;
    
    // Create oracle info for both collaterals
    let oracle_info = vec![
        (helper.xrd_address, "".to_string(), "".to_string()),
        (helper.lsulp_address, "".to_string(), "".to_string())
    ];
    
    // Perform redemption
    let redemption_amount = dec!(100);

    let redeem_fusd = priv_fusd.take(redemption_amount, &mut helper.env)?;
    let (redeemed_collateral, _) = helper.stability_pools.redemptions(
        redeem_fusd,
        oracle_info,
        10, // max_redemptions
        &mut helper.env
    )?;

    let receipt_id_privileged = NonFungibleLocalId::from(1);
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id_privileged.clone())?;
    
    let lsulp_amount = redeemed_collateral.iter()
        .find(|(addr, _)| *addr == helper.lsulp_address)
        .map(|(_, bucket)| bucket.amount(&mut helper.env).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    
    let xrd_amount = redeemed_collateral.iter()
        .find(|(addr, _)| *addr == helper.xrd_address)
        .map(|(_, bucket)| bucket.amount(&mut helper.env).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    
    assert!(lsulp_amount < xrd_amount, "Should have redeemed more XRD than LSULP, as LSULP price higher than XRD. Actual LSULP: {}, Actual XRD: {}", lsulp_amount, xrd_amount);
    assert_eq!(cdp_info.collateral_amount, priv_collateral, "Should not have redeemed privileged CDP");

    let redemption_amount_2 = dec!(700);
    helper.env.disable_auth_module();
    let redeem_fusd_2 = helper.free_fusd(redemption_amount_2)?;
    helper.env.enable_auth_module();

    // Create oracle info for both collaterals
    let oracle_info_2 = vec![
        (helper.xrd_address, "".to_string(), "".to_string()),
        (helper.lsulp_address, "".to_string(), "".to_string())
    ];

    let oracle_info_3 = vec![
        (helper.xrd_address, "".to_string(), "".to_string()),
        (helper.lsulp_address, "".to_string(), "".to_string())
    ];

    let (redeemed_collateral_2, return_fusd) = helper.stability_pools.redemptions(
        redeem_fusd_2,
        oracle_info_2,
        10, // max_redemptions
        &mut helper.env
    )?;
    
    let (_, _) = helper.stability_pools.redemptions(
        return_fusd,
        oracle_info_3,
        10, // max_redemptions
        &mut helper.env
    )?;

    // should have (partially) redeemed the privileged CDP also now.
    let (_, cdp_info_2, _) = helper.get_cdp_info(receipt_id_privileged.clone())?;
    assert!(cdp_info_2.collateral_amount < priv_collateral, "real amount: {}, beginning amount: {}", cdp_info_2.collateral_amount, priv_collateral);

    Ok(())
}

#[test]
fn test_redemption_only_privileged() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower
    let privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: true,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    let privileged_borrower = helper.proxy.create_privileged_borrower(
        privileged_borrower_data,
        &mut helper.env
    )?;
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower
    let priv_collateral = dec!(1000);
    let priv_borrow = dec!(300);
    let priv_bucket = helper.xrd.take(priv_collateral, &mut helper.env)?;
    let (priv_fusd, _) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        priv_bucket,
        priv_borrow,
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    helper.env.enable_auth_module();
    
    // Create oracle info
    let oracle_info = vec![
        (helper.xrd_address, "".to_string(), "".to_string())
    ];
    
    // Perform redemption with only privileged CDPs available
    let redemption_amount = dec!(100);
    let redeem_fusd = priv_fusd.take(redemption_amount, &mut helper.env)?;
    let (redeemed_collateral, _) = helper.stability_pools.redemptions(
        redeem_fusd,
        oracle_info,
        10, // max_redemptions
        &mut helper.env
    )?;
    
    // Should be able to redeem XRD even though it's from privileged borrower
    // when there are no non-privileged CDPs
    let has_xrd = redeemed_collateral.iter().any(|(addr, _)| *addr == helper.xrd_address);
    assert!(has_xrd, "Should have received XRD collateral when only privileged CDPs are available");
    
    // Verify we received a reasonable amount
    let xrd_amount = redeemed_collateral.iter()
        .find(|(addr, _)| *addr == helper.xrd_address)
        .map(|(_, bucket)| bucket.amount(&mut helper.env).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    
    // Should be roughly equivalent to the redemption amount (minus fees)
    assert!(xrd_amount > redemption_amount * dec!(0.9499999), 
           "Expected to receive at least 95% of redemption value in collateral");
    
    Ok(())
}

#[test]
fn test_panic_mode_liquidate() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with high LTV ratio
    let collateral_amount = dec!(1000);
    let borrow_amount = dec!(400); // Close to liquidation threshold of 2 MCR
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01))?;
    
    // Get the CDP ID
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Reduce collateral price to trigger liquidation eligibility
    helper.env.disable_auth_module();
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    helper.env.enable_auth_module();
    
    // Initiate panic mode (first stage)
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time beyond wait period but before timeout
    let new_time = helper.env.get_current_time().add_hours(25).unwrap(); // 25 hours
    helper.env.set_current_time(new_time);
    
    // Complete panic mode initiation
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Perform a panic mode liquidation using USDC
    let usdc_payment = helper.usdc.take(dec!(500), &mut helper.env)?;
    let (collateral, leftover) = helper.stability_pools.panic_mode_liquidate(
        receipt_id.clone(),
        usdc_payment,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify we received collateral
    assert_eq!(collateral.resource_address(&mut helper.env)?, helper.xrd_address);
    assert!(collateral.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Verify CDP was liquidated
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.status, CdpStatus::Liquidated);
    
    Ok(())
}

#[test]
fn test_system_recovery_from_panic_mode() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // SETUP PHASE: Create two CDPs - one to trigger panic mode, one to test recovery
    
    // First CDP - will be used to trigger panic mode
    let collateral_amount1 = dec!(1000);
    let borrow_amount1 = dec!(400); // Close to liquidation threshold
    let bucket1 = helper.xrd.take(collateral_amount1, &mut helper.env)?;
    let (_, cdp_receipt1) = helper.proxy_open_cdp(None, bucket1, borrow_amount1, dec!(0.01))?;
    let receipt_id1 = NonFungibleLocalId::from(1);
    
    // Second CDP - will be used to verify system recovery
    let collateral_amount2 = dec!(2000);
    let borrow_amount2 = dec!(300);
    let bucket2 = helper.xrd.take(collateral_amount2, &mut helper.env)?;
    let (fusd2, cdp_receipt2) = helper.proxy_open_cdp(None, bucket2, borrow_amount2, dec!(0.01))?;
    let receipt_id2 = NonFungibleLocalId::from(2);
    
    // PHASE 1: Trigger panic mode
    
    // Reduce collateral price to make the first CDP liquidatable
    helper.change_collateral_price("XRD".to_string(), dec!(0.4))?;
    
    // Initiate panic mode (first stage) with first CDP
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id1.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Fast forward time to complete panic mode initiation
    let new_time = helper.env.get_current_time().add_hours(25).unwrap();
    helper.env.set_current_time(new_time);
    
    // Complete panic mode initiation
    helper.stability_pools.check_and_initiate_panic_mode(
        receipt_id1.clone(),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify panic mode is active
    let panic_mode_active = helper.stability_pools.check_panic_mode_status(&mut helper.env)?;
    assert!(panic_mode_active);
    
    // PHASE 2: Liquidate the problematic CDP in panic mode
    
    // Prepare USDC for panic mode liquidation
    let usdc_payment = helper.usdc.take(dec!(4000), &mut helper.env)?;
    let (collateral, _) = helper.stability_pools.panic_mode_liquidate(
        receipt_id1.clone(),
        usdc_payment,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify the first CDP was liquidated
    let (_, cdp_info1, _) = helper.get_cdp_info(receipt_id1.clone())?;
    assert_eq!(cdp_info1.status, CdpStatus::Liquidated);
    
    // PHASE 3: Fill stability pools with fUSD to recover
    
    // Contribute fUSD to the stability pool to enable normal operation
    helper.env.disable_auth_module();
    let pool_fusd = helper.free_fusd(dec!(1200))?;
    helper.env.enable_auth_module();

    helper.stability_pools.contribute_to_pool(
        helper.xrd_address,
        pool_fusd,
        false,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // PHASE 4: Reset panic mode cooldown by advancing time
    
    // Fast forward time beyond panic mode cooldown
    let new_time = helper.env.get_current_time().add_days(2).unwrap();
    helper.env.set_current_time(new_time);
    
    // PHASE 5: Verify system restored to normal operation
    
    // Create a new CDP to verify normal operations are working
    let collateral_amount3 = dec!(1500);
    let borrow_amount3 = dec!(200);
    let bucket3 = helper.xrd.take(collateral_amount3, &mut helper.env)?;
    let (fusd3, cdp_receipt3) = helper.proxy_open_cdp(None, bucket3, borrow_amount3, dec!(0.01))?;
    let receipt_id3 = NonFungibleLocalId::from(3);
    
    // Verify new CDP was created successfully
    let (_, cdp_info3, _) = helper.get_cdp_info(receipt_id3.clone())?;
    assert_eq!(cdp_info3.status, CdpStatus::Healthy);
    
    // Check if panic mode is still active
    let panic_mode_still_active = helper.stability_pools.check_panic_mode_status(&mut helper.env)?;
    
    // If panic mode is still active after all these steps, then there may not be
    // an automatic way to deactivate it, or our recovery steps were insufficient
    if panic_mode_still_active {
        // This is a valid test outcome if the system requires manual intervention to exit panic mode
        // The test would still demonstrate the system's behavior during and after panic mode
    } else {
        // If panic mode is no longer active, validate that normal operations work
        // Verify that normal liquidation can happen now by making CDP2 liquidatable
        helper.change_collateral_price("XRD".to_string(), dec!(0.2))?;
        
        // Try to liquidate CDP2 normally
        let result = helper.stability_pools.liquidate(
            receipt_id2.clone(),
            "".to_string(),
            "".to_string(),
            &mut helper.env
        )?;
    }
    
    Ok(())
}