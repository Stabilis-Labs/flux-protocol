mod helper;
use helper::Helper;
use flux_protocol::shared_structs::*;

use scrypto_test::prelude::*;
use scrypto::prelude::Url;

#[test]
fn test_proxy_open_cdp() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;

    // Put tokens into the DAO
    let (fusd, _cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01))?;

    let (id, cdp_info, real_debt_modifier) = helper.get_cdp_info(NonFungibleLocalId::from(1))?;

    let extra_interest = (Decimal::ONE + (dec!(0.01) / dec!(31_556_926)))
                .checked_powi(7 as i64 * 86400)
                .unwrap()
                * dec!(10)
                - dec!(10);

    let expected_debt = dec!(10) + extra_interest;

    assert_eq!(fusd.amount(&mut helper.env)?, dec!(10));
    assert_eq!(id, NonFungibleLocalId::from(1));
    assert_eq!(expected_debt, real_debt_modifier * cdp_info.pool_debt);
    assert_eq!(cdp_info.status, CdpStatus::Healthy);
    assert_eq!(cdp_info.privileged_borrower, None);
    assert_eq!(cdp_info.collateral_address, helper.xrd_address);
    assert_eq!(cdp_info.collateral_amount, dec!(1000));
    assert_eq!(cdp_info.interest, dec!(0.01));

    Ok(())
}



#[test]
fn test_proxy_close_cdp() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
    let bucket_2 = helper.xrd.take(dec!(1000), &mut helper.env)?;

    // Open a CDP
    let (fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    let (fusd_2, cdp_receipt_2) = helper.proxy_open_cdp(None, bucket_2, dec!(50), dec!(0.01))?;

    fusd.put(fusd_2.take(dec!(50), &mut helper.env)?, &mut helper.env)?;
    
    // Get the ID of the cdp receipt for verification
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Close the CDP using the proxy component with proof
    let (collateral, _) = helper.proxy.close_cdp(receipt_proof, fusd, &mut helper.env)?;
    
    // Verify the collateral is returned correctly
    assert_eq!(collateral.resource_address(&mut helper.env)?, helper.xrd_address);
    assert_eq!(collateral.amount(&mut helper.env)?, dec!(1000));
    
    let (_, updated_cdp_info, updated_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    
    // There should be no CDPs
    assert_eq!(updated_cdp_info.status, CdpStatus::Closed);

    Ok(())
}



#[test]
fn test_proxy_partial_close_cdp() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;

    // Open a CDP
    let (fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(100), dec!(0.01))?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, real_debt_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let initial_debt = real_debt_modifier * initial_cdp_info.pool_debt;
    
    // Only pay back half of the debt
    let repayment_amount = dec!(50);
    let remaining_amount = initial_debt - repayment_amount;
    
    // Split the fusd bucket to partially repay
    let repayment = fusd.take(repayment_amount, &mut helper.env)?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Partial close the CDP using the proxy component with proof
    let (collateral_bucket, leftover_fusd) = helper.proxy.partial_close_cdp(receipt_proof, repayment, &mut helper.env)?;
    
    assert!(collateral_bucket.is_none());
    assert!(leftover_fusd.is_none());
    
    // Check that the CDP still exists with updated values
    let (_, updated_cdp_info, updated_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let new_debt = updated_modifier * updated_cdp_info.pool_debt;
    
    // Assert that the debt has been reduced (approximately)
    let debt_difference = if new_debt > remaining_amount {
        new_debt - remaining_amount
    } else {
        remaining_amount - new_debt
    };
    
    assert!(
        debt_difference <= dec!(0),
        "Expected remaining debt around {}, but got {}",
        remaining_amount,
        new_debt
    );

    Ok(())
}



#[test]
fn test_top_up_cdp() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Initial collateral and additional top-up amount
    let initial_collateral = dec!(1000);
    let top_up_amount = dec!(500);
    
    // Create initial buckets
    let bucket = helper.xrd.take(initial_collateral, &mut helper.env)?;
    let top_up_bucket = helper.xrd.take(top_up_amount, &mut helper.env)?;
    
    // Open a CDP
    let (_fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(initial_cdp_info.collateral_amount, initial_collateral);
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Top up the CDP using the proxy component with proof
    helper.proxy.top_up_cdp(receipt_proof, top_up_bucket, "".to_string(), "".to_string(), &mut helper.env)?;
    
    // Get updated CDP info
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    // Assert that collateral amount has increased
    assert_eq!(
        updated_cdp_info.collateral_amount, 
        initial_collateral + top_up_amount
    );
    
    // Keep the same debt amount
    assert_eq!(updated_cdp_info.pool_debt, initial_cdp_info.pool_debt);

    Ok(())
}



#[test]
fn test_remove_collateral() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Initial collateral
    let initial_collateral = dec!(2000);
    let remove_amount = dec!(500);
    
    // Create initial bucket
    let bucket = helper.xrd.take(initial_collateral, &mut helper.env)?;
    
    // Open a CDP with a small debt relative to collateral
    let (_fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(initial_cdp_info.collateral_amount, initial_collateral);
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Remove some collateral using proxy component with proof
    let removed_collateral = helper.proxy.remove_collateral(
        receipt_proof, 
        remove_amount, 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    )?;
    
    // Check that we received the correct amount of collateral
    assert_eq!(removed_collateral.amount(&mut helper.env)?, remove_amount);
    assert_eq!(removed_collateral.resource_address(&mut helper.env)?, helper.xrd_address);
    
    // Get updated CDP info
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    // Assert that collateral amount has decreased
    assert_eq!(
        updated_cdp_info.collateral_amount, 
        initial_collateral - remove_amount
    );
    
    // Debt should remain the same
    assert_eq!(updated_cdp_info.pool_debt, initial_cdp_info.pool_debt);

    Ok(())
}

#[test]
fn test_change_cdp_interest_after_cooldown() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(2000);
    let initial_interest = dec!(0.01); // 1%
    let new_interest = dec!(0.02);     // 2%
    
    // Create a CDP
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), initial_interest)?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    let initial_pool_debt = initial_cdp_info.pool_debt;
    
    // Advance time by 10 days (beyond the cooldown period)
    let new_time = helper.env.get_current_time().add_days(10).unwrap();
    helper.env.set_current_time(new_time);
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Change interest rate after cooldown
    helper.proxy.change_cdp_interest(
        receipt_proof, 
        None, 
        new_interest, 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    )?;
    
    // Get updated CDP info
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    let actual_debt_increase = updated_cdp_info.pool_debt - initial_pool_debt;
    let expected_interest = dec!(0);

    // The actual debt increase should be close to expected interest (small margin for error)
    let error_margin = dec!(0.0001);
    assert!(
        (actual_debt_increase - expected_interest).checked_abs().unwrap() <= error_margin, 
        "Expected debt increase to be approximately {}, but got {}",
        expected_interest,
        actual_debt_increase
    );
    
    Ok(())
}

#[test]
fn test_remove_too_much_collateral() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Initial collateral
    let initial_collateral = dec!(2000);
    let remove_amount = dec!(1999);
    
    // Create initial bucket
    let bucket = helper.xrd.take(initial_collateral, &mut helper.env)?;
    
    // Open a CDP with a small debt relative to collateral
    let (_fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(initial_cdp_info.collateral_amount, initial_collateral);
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Remove some collateral using proxy component with proof - this should fail
    let result = helper.proxy.remove_collateral(
        receipt_proof, 
        remove_amount, 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    );
    
    // Assert that the operation failed
    assert!(result.is_err(), "Removing too much collateral should fail");

    Ok(())
}



#[test]
fn test_borrow_more() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(2000);
    let initial_borrow = dec!(50);
    let additional_borrow = dec!(30);
    let additional_debt = additional_borrow + (Decimal::ONE + (dec!(0.01) / dec!(31_556_926)))
        .checked_powi(7 * 86400)
        .unwrap()
        * dec!(30)
        - dec!(30);
    
    // Create initial bucket
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    
    // Open a CDP
    let (_initial_fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, initial_borrow, dec!(0.01))?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, initial_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let initial_debt = initial_modifier * initial_cdp_info.pool_debt;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Borrow more using proxy component with proof
    let additional_fusd = helper.proxy.borrow_more(
        receipt_proof, 
        additional_borrow, 
        "".to_string(),
        "".to_string(), 
        &mut helper.env
    )?;
    
    // Check that we received the correct amount of additional fUSD
    assert_eq!(additional_fusd.amount(&mut helper.env)?, additional_borrow);
    
    // Get updated CDP info
    let (_, updated_cdp_info, updated_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let updated_debt = updated_modifier * updated_cdp_info.pool_debt;

    assert_eq!(updated_debt, initial_debt + additional_debt);
    assert_eq!(updated_cdp_info.collateral_amount, initial_cdp_info.collateral_amount);

    Ok(())
}



#[test]
fn test_change_cdp_interest() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(2000);
    let initial_interest = dec!(0.01); // 1%
    let new_interest = dec!(0.02);     // 2%
    
    // Create initial bucket
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    
    // Open a CDP with initial interest rate
    let (_fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), initial_interest)?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    let initial_pool_debt = initial_cdp_info.pool_debt;
    assert_eq!(initial_cdp_info.interest, initial_interest);
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Change interest rate using proxy component with proof
    helper.proxy.change_cdp_interest(
        receipt_proof, 
        None, 
        new_interest, 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    )?;
    
    // Get updated CDP info
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    // Assert that interest rate has changed
    assert_eq!(updated_cdp_info.interest, new_interest);
    // Assert debt went up
    assert!(initial_pool_debt < updated_cdp_info.pool_debt);

    Ok(())
}



#[test]
fn test_open_cdp_with_lsulp() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of LSULP tokens
    let mut helper = Helper::new().unwrap();
    let lsulp_amount = dec!(1000);
    let bucket = helper.lsulp.take(lsulp_amount, &mut helper.env)?;
    let bucket_2 = helper.lsulp.take(lsulp_amount, &mut helper.env)?;

    // Open a CDP with LSULP collateral
    let (fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01))?;
    let (fusd_2, cdp_receipt_2) = helper.proxy_open_cdp(None, bucket_2, dec!(10), dec!(0.01))?;

    fusd.put(fusd_2.take(dec!(10), &mut helper.env)?, &mut helper.env)?;

    // Get the CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (id, cdp_info, real_debt_modifier) = helper.get_cdp_info(receipt_id.clone())?;

    // Calculate expected debt including interest
    let extra_interest = (Decimal::ONE + (dec!(0.01) / dec!(31_556_926)))
                .checked_powi(7 as i64 * 86400)
                .unwrap()
                * dec!(10)
                - dec!(10);
    let expected_debt = dec!(10) + extra_interest;

    // Verify the CDP details
    assert_eq!(fusd.amount(&mut helper.env)?, dec!(20));
    assert_eq!(id, NonFungibleLocalId::from(1));
    assert_eq!(expected_debt, real_debt_modifier * cdp_info.pool_debt);
    assert_eq!(cdp_info.status, CdpStatus::Healthy);
    assert_eq!(cdp_info.privileged_borrower, None);
    
    // Most importantly, verify that the collateral is LSULP, not XRD
    assert_eq!(cdp_info.collateral_address, helper.lsulp_address);
    assert_eq!(cdp_info.collateral_amount, lsulp_amount);
    assert_eq!(cdp_info.interest, dec!(0.01));

    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Close the CDP using proxy component with proof
    let (collateral, _) = helper.proxy.close_cdp(receipt_proof, fusd, &mut helper.env)?;
    
    // Verify that we got LSULP back, not XRD
    assert_eq!(collateral.resource_address(&mut helper.env)?, helper.lsulp_address);
    assert_eq!(collateral.amount(&mut helper.env)?, lsulp_amount);

    Ok(())
}



#[test]
fn test_open_cdp_with_too_little_collateral() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Create a very small amount of collateral (MCR is 2, so this is too little for the borrowing amount)
    let collateral_amount = dec!(10);
    let borrow_amount = dec!(10); // Needs 2x collateral (20) based on MCR but only has 10
    let bucket = helper.xrd.take(collateral_amount, &mut helper.env)?;
    
    // Attempt to open a CDP with insufficient collateral - should fail
    let result = helper.proxy_open_cdp(None, bucket, borrow_amount, dec!(0.01));
    
    // Verify that opening CDP with too little collateral fails
    assert!(result.is_err(), "Opening CDP with insufficient collateral should fail");
    
    Ok(())
}

#[test]
fn test_open_cdp_with_non_accepted_collateral() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of USDC tokens (which is not accepted as collateral)
    let mut helper = Helper::new().unwrap();
    let usdc_amount = dec!(1000);
    let bucket = helper.usdc.take(usdc_amount, &mut helper.env)?;
    
    // Attempt to open a CDP with non-accepted collateral - should fail
    let result = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01));
    
    // Verify that opening CDP with non-accepted collateral fails
    assert!(result.is_err(), "Opening CDP with non-accepted collateral should fail");
    
    Ok(())
}

#[test]
fn test_close_cdp_with_wrong_token() -> Result<(), RuntimeError> {
    // Initialize helper and create buckets
    let mut helper = Helper::new().unwrap();
    let xrd_amount = dec!(1000);
    let bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    
    // Open a CDP with XRD collateral
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Create wrong token - using USDC instead of fUSD
    let wrong_token = helper.usdc.take(dec!(60), &mut helper.env)?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Attempt to close CDP with wrong token - should fail
    let result = helper.proxy.close_cdp(receipt_proof, wrong_token, &mut helper.env);
    
    // Verify that closing with wrong token fails
    assert!(result.is_err(), "Closing CDP with wrong token should fail");
    
    Ok(())
}



#[test]
fn test_top_up_cdp_with_wrong_resource() -> Result<(), RuntimeError> {
    // Initialize helper and create buckets
    let mut helper = Helper::new().unwrap();
    let xrd_amount = dec!(1000);
    let bucket = helper.xrd.take(xrd_amount, &mut helper.env)?;
    
    // Open a CDP with XRD collateral
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Create a different token to top up with (LSULP)
    let wrong_collateral = helper.lsulp.take(dec!(500), &mut helper.env)?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Attempt to top up with wrong resource - should fail
    let result = helper.proxy.top_up_cdp(
        receipt_proof, 
        wrong_collateral, 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    );
    
    // Verify that topping up with wrong resource fails
    assert!(result.is_err(), "Topping up CDP with wrong resource should fail");
    
    Ok(())
}



#[test]
fn test_set_max_vector_length() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Now set max vector length to 3
    helper.env.disable_auth_module();
    helper.proxy.set_max_vector_length(3, &mut helper.env)?;
    helper.env.enable_auth_module();
    
    // Attempt to create more CDPs beyond the limit - should fail on the 4th one
    for i in 0..4 {
        let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
        let result = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01));
        
        if i < 3 {
            // First 3 should succeed
            assert!(result.is_ok(), "Creating CDP #{} should succeed", i+1);
        } else {
            // 4th should fail
            assert!(result.is_err(), "Creating CDP #{} should fail due to max vector length", i+1);
        }
    }
    
    Ok(())
}



#[test]
fn test_edit_collateral_acceptance() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // First verify we can open a CDP with XRD
    let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
    let result = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01));
    assert!(result.is_ok(), "Should be able to open a CDP with XRD initially");
    
    // Now edit the XRD collateral to set acceptance to false
    helper.env.disable_auth_module();
    helper.proxy.edit_collateral(
        helper.xrd_address,
        dec!(2), // keep same MCR
        false,   // set acceptance to false
        &mut helper.env
    )?;
    helper.env.enable_auth_module();
    
    // Try to open a CDP with XRD again - should fail
    let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
    let result = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01));
    assert!(result.is_err(), "Should not be able to open a CDP with XRD after setting acceptance to false");
    
    Ok(())
}



#[test]
fn test_borrow_more_after_acceptance_false() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Open a CDP with XRD
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01))?;
    
    // Now edit the XRD collateral to set acceptance to false
    helper.env.disable_auth_module();
    helper.proxy.edit_collateral(
        helper.xrd_address,
        dec!(2), // keep same MCR
        false,   // set acceptance to false
        &mut helper.env
    )?;
    helper.env.enable_auth_module();
    
    // Create a proof and try to borrow more - should fail
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    let result = helper.proxy.borrow_more(
        receipt_proof,
        dec!(10),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    );
    
    assert!(result.is_err(), "Should not be able to borrow more after setting acceptance to false");
    
    Ok(())
}



#[test]
fn test_edit_collateral_mcr() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // First verify we can open a CDP with XRD at standard MCR (2)
    let bucket = helper.xrd.take(dec!(200), &mut helper.env)?;
    let result = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01)); // 200/50 = 4 > MCR of 2
    assert!(result.is_ok(), "Should be able to open a CDP with XRD at standard MCR");
    
    // Now edit the XRD collateral to set MCR to something very high
    helper.env.disable_auth_module();
    helper.proxy.edit_collateral(
        helper.xrd_address,
        dec!(10), // set MCR very high
        true,     // keep acceptance true
        &mut helper.env
    )?;
    helper.env.enable_auth_module();

    // But should be able to open with more collateral to satisfy the new MCR
    let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
    let result = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01)); // 1000/50 = 20 > new MCR of 10
    assert!(result.is_ok(), "Should be able to open a CDP with XRD with enough collateral for new MCR");
    
    // Try to open a CDP with XRD again with same ratio - should fail
    let bucket = helper.xrd.take(dec!(200), &mut helper.env)?;
    let result = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01)); // 200/50 = 4 < new MCR of 10
    assert!(result.is_err(), "Should not be able to open a CDP with XRD after increasing MCR");
    
    Ok(())
}



#[test]
fn test_borrow_more_than_allowed() -> Result<(), RuntimeError> {
    // Initialize helper and create a bucket of XRD tokens
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(1000);
    let initial_borrow = dec!(100);
    let additional_borrow = dec!(900); // This would exceed the MCR (2) since 1000/1000 = 1 < 2
    
    // Create initial bucket
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    
    // Open a CDP
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, initial_borrow, dec!(0.01))?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Attempt to borrow more than allowed - should fail
    let result = helper.proxy.borrow_more(
        receipt_proof, 
        additional_borrow, 
        "".to_string(),
        "".to_string(), 
        &mut helper.env
    );
    
    // Verify that borrowing too much fails
    assert!(result.is_err(), "Borrowing more than allowed by MCR should fail");
    
    Ok(())
}


#[test]
fn test_tag_irredeemable() -> Result<(), RuntimeError> {
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
    
    // Open a CDP as a privileged borrower with special negative interest rate
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        dec!(100),
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify the CDP has the negative interest rate
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.interest, dec!(-420));
    
    // Now modify the privileged borrower to disallow redemption opt-out
    let privileged_borrower_id = NonFungibleLocalId::from(1); // First privileged borrower created
    let updated_privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: false,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    helper.proxy.edit_privileged_borrower(
        updated_privileged_borrower_data,
        privileged_borrower_id,
        &mut helper.env
    )?;
    
    // Tag the CDP as irredeemable
    let fusd_reward = helper.proxy.tag_irredeemable(receipt_id.clone(), &mut helper.env)?;
    helper.env.enable_auth_module();
    
    // Verify we received payment for tagging
    assert!(fusd_reward.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Verify the CDP status has been updated
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    // CDP should not be irredeemable anymore
    assert!(updated_cdp_info.interest != dec!(-420));
    
    Ok(())
}

#[test]
fn test_partial_close_cdp_full_repayment() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(1000);
    let borrowed_amount = dec!(100);
    
    // Create a CDP
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    let (fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrowed_amount, dec!(0.01))?;
    
    // Get CDP info to calculate total debt
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, cdp_info, real_debt_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let total_debt = real_debt_modifier * cdp_info.pool_debt;
    
    // Get a bit more fUSD to cover interest
    helper.env.disable_auth_module();
    let extra_fusd = helper.free_fusd(dec!(10))?;
    helper.env.enable_auth_module();
    fusd.put(extra_fusd, &mut helper.env)?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Use partial_close but pay back full amount - should behave like close_cdp
    let (collateral_returned, leftover_fusd) = helper.proxy.partial_close_cdp(
        receipt_proof,
        fusd,
        &mut helper.env
    )?;
    
    // Verify we got collateral back
    assert!(collateral_returned.is_some());
    let collateral_bucket = collateral_returned.unwrap();
    assert_eq!(collateral_bucket.resource_address(&mut helper.env)?, helper.xrd_address);
    assert_eq!(collateral_bucket.amount(&mut helper.env)?, collateral);
    
    // Verify we got leftover fUSD back
    assert!(leftover_fusd.is_some());
    let leftover = leftover_fusd.unwrap();
    assert!(leftover.amount(&mut helper.env)? > dec!(0));
    
    // Verify the CDP is closed
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(updated_cdp_info.status, CdpStatus::Closed);
    
    Ok(())
}



#[test]
fn test_partial_close_cdp_with_wrong_token() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(1000);
    let borrowed_amount = dec!(100);
    
    // Create a CDP
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrowed_amount, dec!(0.01))?;
    
    // Create wrong token (USDC instead of fUSD)
    let wrong_token = helper.usdc.take(dec!(50), &mut helper.env)?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Try to partial close with wrong token - should fail
    let result = helper.proxy.partial_close_cdp(
        receipt_proof,
        wrong_token,
        &mut helper.env
    );
    
    // Verify the operation failed
    assert!(result.is_err(), "Partial closing CDP with wrong token should fail");
    
    Ok(())
}



#[test]
fn test_privileged_borrower_open_cdp() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower with fixed interest rate capability
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
    helper.env.enable_auth_module();
    
    // Create a proof from the privileged borrower NFT
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);
    
    // Open a CDP as a privileged borrower with special negative interest rate
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    let (fusd, cdp_receipt) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        dec!(100),
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify the CDP has the negative interest rate
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    // Verify the privileged borrower can use the special negative interest rate
    assert_eq!(cdp_info.interest, dec!(-420));
    
    // Check that the debt behavior is as expected with negative interest
    assert_eq!(cdp_info.pool_debt, dec!(100));
    
    Ok(())
}

#[test]
fn test_partial_close_cdp_partial_repayment() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Initial setup
    let collateral = dec!(1000);
    let borrowed_amount = dec!(100);
    
    // Create a CDP
    let bucket = helper.xrd.take(collateral, &mut helper.env)?;
    let (fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, borrowed_amount, dec!(0.01))?;
    
    // Get initial CDP info
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, initial_cdp_info, initial_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let initial_debt = initial_modifier * initial_cdp_info.pool_debt;
    
    // Only pay back half of the debt
    let repayment_amount = dec!(50);
    let remaining_amount = initial_debt - repayment_amount;
    
    // Split the fusd bucket to partially repay
    let repayment = fusd.take(repayment_amount, &mut helper.env)?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Partial close the CDP
    let (collateral_bucket, leftover_fusd) = helper.proxy.partial_close_cdp(
        receipt_proof,
        repayment,
        &mut helper.env
    )?;
    
    // For a partial repayment, we shouldn't get any collateral or leftover fUSD back
    assert!(collateral_bucket.is_none());
    assert!(leftover_fusd.is_none());
    
    // Get updated CDP info
    let (_, updated_cdp_info, updated_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let updated_debt = updated_modifier * updated_cdp_info.pool_debt;
    
    // The debt should be reduced (approximately) by the repayment amount
    let debt_difference = if initial_debt > updated_debt {
        initial_debt - updated_debt
    } else {
        updated_debt - initial_debt
    };
    
    // Allow for small rounding differences
    let tolerance = dec!(0.0001);
    assert!(
        (debt_difference - repayment_amount).checked_abs().unwrap() <= tolerance,
        "Expected debt to be reduced by approximately {}, but was reduced by {}",
        repayment_amount,
        debt_difference
    );
    
    // The CDP should still be open
    assert_eq!(updated_cdp_info.status, CdpStatus::Healthy);
    
    // The collateral amount should remain unchanged
    assert_eq!(updated_cdp_info.collateral_amount, initial_cdp_info.collateral_amount);
    
    Ok(())
}

#[test]
fn test_tag_irredeemable_cdp() -> Result<(), RuntimeError> {
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
    
    // Open a CDP as a privileged borrower with special negative interest rate
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy.open_cdp(
        Some(privileged_borrower_proof),
        bucket,
        dec!(100),
        dec!(-420),
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // Verify the CDP has the negative interest rate
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(cdp_info.interest, dec!(-420));
    
    // Now modify the privileged borrower to disallow redemption opt-out
    let privileged_borrower_id = NonFungibleLocalId::from(1); // First privileged borrower created
    let updated_privileged_borrower_data = PrivilegedBorrowerData {
        redemption_opt_out: false,
        liquidation_notice: Some(1),
        max_coupled_loans: 10,
        coupled_loans: vec![],
        key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
    };
    
    helper.proxy.edit_privileged_borrower(
        updated_privileged_borrower_data,
        privileged_borrower_id,
        &mut helper.env
    )?;
    
    // Tag the CDP as irredeemable
    let fusd_reward = helper.proxy.tag_irredeemable(receipt_id.clone(), &mut helper.env)?;
    helper.env.enable_auth_module();
    
    // Verify we received payment for tagging
    assert!(fusd_reward.amount(&mut helper.env)? > Decimal::ZERO);
    
    // Verify the CDP status has been updated
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    
    // CDP should not be irredeemable anymore
    assert!(updated_cdp_info.interest != dec!(-420));
    
    Ok(())
}

#[test]
fn test_change_cdp_interest_with_privileged_borrower() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Disable auth module to create a privileged borrower
    helper.env.disable_auth_module();
    
    // Create a privileged borrower with fixed interest rate capability
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
    helper.env.enable_auth_module();
    
    // Create a CDP with normal interest rate first
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Create a proof from the receipt and privileged borrower
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    let privileged_borrower_proof = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);

    helper.proxy.link_cdp_to_privileged_borrower(privileged_borrower_proof,receipt_proof, &mut helper.env)?;

    let receipt_proof_2 = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    let privileged_borrower_proof_2 = NonFungibleProof(privileged_borrower.create_proof_of_all(&mut helper.env)?);

    // Change to privileged negative interest rate
    helper.proxy.change_cdp_interest(
        receipt_proof_2, 
        Some(privileged_borrower_proof_2), 
        dec!(-420), 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    )?;
    
    // Verify the interest rate changed
    let receipt_id = NonFungibleLocalId::from(1);
    let (_, updated_cdp_info, _) = helper.get_cdp_info(receipt_id)?;
    assert_eq!(updated_cdp_info.interest, dec!(-420));
    
    Ok(())
}

#[test]
fn test_change_cdp_interest_with_non_privileged_borrower() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create a CDP with normal interest rate
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    let (_, cdp_receipt) = helper.proxy_open_cdp(None, bucket, dec!(50), dec!(0.01))?;
    
    // Create a proof from the receipt
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    
    // Attempt to change to negative interest rate without privilege - should fail
    let result = helper.proxy.change_cdp_interest(
        receipt_proof, 
        None, 
        dec!(-420), 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    );
    
    // Verify that changing to negative interest without privilege fails
    assert!(result.is_err(), "Should not be able to set negative interest without privilege");
    
    Ok(())
}

#[test]
fn test_non_privileged_open_cdp_negative_interest() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create bucket of XRD tokens
    let bucket = helper.xrd.take(dec!(2000), &mut helper.env)?;
    
    // Try to open a CDP with negative interest rate without privilege - should fail
    let result = helper.proxy_open_cdp(None, bucket, dec!(100), dec!(-420));
    
    // Verify that opening with negative interest without privilege fails
    assert!(result.is_err(), "Should not be able to open CDP with negative interest without privilege");
    
    Ok(())
}

#[test]
fn test_max_vector_length() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // Create 3 identical CDPs, which should work by default
    for _ in 0..3 {
        let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
        let (_, _) = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01))?;
    }
    
    // Now set max vector length to 3
    helper.env.disable_auth_module();
    helper.proxy.set_max_vector_length(3, &mut helper.env)?;
    helper.env.enable_auth_module();
    
    // The 4th CDP creation should fail due to vector length limit
    let bucket = helper.xrd.take(dec!(1000), &mut helper.env)?;
    let result = helper.proxy_open_cdp(None, bucket, dec!(10), dec!(0.01));
    
    // Verify that creating beyond the vector length limit fails
    assert!(result.is_err(), "Creating CDP beyond max vector length should fail");
    
    Ok(())
}

#[test]
fn test_compound_realistic_user_operations() -> Result<(), RuntimeError> {
    // Initialize helper
    let mut helper = Helper::new().unwrap();
    
    // PHASE 1: User opens a CDP with XRD collateral
    let initial_collateral = dec!(1000);
    let initial_borrow = dec!(200);
    let bucket = helper.xrd.take(initial_collateral, &mut helper.env)?;
    let (mut fusd, cdp_receipt) = helper.proxy_open_cdp(None, bucket, initial_borrow, dec!(0.01))?;
    let receipt_id = NonFungibleLocalId::from(1);
    
    // Check initial CDP state
    let (_, initial_cdp_info, initial_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let initial_debt = initial_modifier * initial_cdp_info.pool_debt;
    
    // PHASE 2: Market conditions change, XRD price increases
    helper.change_collateral_price("XRD".to_string(), dec!(1.5))?;
    
    // PHASE 3: User borrows more due to increased collateral value
    let receipt_proof = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    let additional_borrow = dec!(100);
    let more_fusd = helper.proxy.borrow_more(
        receipt_proof, 
        additional_borrow, 
        "".to_string(),
        "".to_string(), 
        &mut helper.env
    )?;
    fusd.put(more_fusd, &mut helper.env)?;
    
    // PHASE 4: Time passes, interest accrues
    let new_time = helper.env.get_current_time().add_days(30).unwrap();
    helper.env.set_current_time(new_time);
    
    // Check CDP state after borrowing more and interest accrual
    let (_, mid_cdp_info, mid_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let mid_debt = mid_modifier * mid_cdp_info.pool_debt;
    assert!(mid_debt > initial_debt + additional_borrow, "Debt should increase due to interest");
    
    // PHASE 5: Market conditions change, XRD price decreases
    helper.change_collateral_price("XRD".to_string(), dec!(0.9))?;
    
    // PHASE 6: User adds more collateral to avoid liquidation
    let receipt_proof_2 = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    let top_up_amount = dec!(500);
    let top_up_bucket = helper.xrd.take(top_up_amount, &mut helper.env)?;
    helper.proxy.top_up_cdp(
        receipt_proof_2,
        top_up_bucket,
        "".to_string(),
        "".to_string(),
        &mut helper.env
    )?;
    
    // PHASE 7: User repays part of the loan
    let receipt_proof_3 = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    let repayment_amount = dec!(150);
    let repayment = fusd.take(repayment_amount, &mut helper.env)?;
    helper.proxy.partial_close_cdp(
        receipt_proof_3,
        repayment,
        &mut helper.env
    )?;
    
    // Check CDP state after partial repayment
    let (_, after_repay_cdp_info, after_repay_modifier) = helper.get_cdp_info(receipt_id.clone())?;
    let after_repay_debt = after_repay_modifier * after_repay_cdp_info.pool_debt;
    assert!(after_repay_debt < mid_debt, "Debt should decrease after partial repayment");
    
    // PHASE 8: User reduces interest rate after cooldown
    let new_time = helper.env.get_current_time().add_days(10).unwrap();
    helper.env.set_current_time(new_time);
    let receipt_proof_4 = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    helper.proxy.change_cdp_interest(
        receipt_proof_4, 
        None, 
        dec!(0.005), // Lower interest rate
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    )?;
    
    // Check that interest rate changed
    let (_, final_cdp_info, _) = helper.get_cdp_info(receipt_id.clone())?;
    assert_eq!(final_cdp_info.interest, dec!(0.005), "Interest rate should be updated");
    
    // PHASE 9: User removes some collateral as market stabilizes
    let receipt_proof_5 = NonFungibleProof(cdp_receipt.create_proof_of_all(&mut helper.env)?);
    helper.change_collateral_price("XRD".to_string(), dec!(1.2))?;
    
    let remove_amount = dec!(200);
    let removed_collateral = helper.proxy.remove_collateral(
        receipt_proof_5, 
        remove_amount, 
        "".to_string(), 
        "".to_string(), 
        &mut helper.env
    )?;
    
    assert_eq!(removed_collateral.amount(&mut helper.env)?, remove_amount);
    
    // Verify final CDP state
    let (_, final_cdp_info, final_modifier) = helper.get_cdp_info(receipt_id)?;
    assert_eq!(final_cdp_info.collateral_amount, initial_collateral + top_up_amount - remove_amount);
    assert_eq!(final_cdp_info.status, CdpStatus::Healthy);
    
    Ok(())
}
