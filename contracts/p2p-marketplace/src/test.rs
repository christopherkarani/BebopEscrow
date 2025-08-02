#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as TestAddress, Ledger, LedgerInfo},
    token, Address, Env,
};

// Helper function to create a token contract for testing
fn create_token_contract<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, token::Client<'a>) {
    let stellar_asset = env.register_stellar_asset_contract_v2(admin.clone());
    let contract_address = stellar_asset.address();
    let client = token::Client::new(env, &contract_address);
    (contract_address, client)
}

// Helper function to setup token balance for testing
fn setup_token_balance(env: &Env, token_admin: &Address, token_id: &Address, user: &Address, amount: i128, marketplace_contract: &Address) {
    let token_admin_client = token::StellarAssetClient::new(env, token_id);
    token_admin_client.mint(user, &amount);
    
    // Also set up allowance for the marketplace contract
    let token_client = token::Client::new(env, token_id);
    token_client.approve(user, marketplace_contract, &amount, &99999);
}

// Main test setup function
fn setup_test_env() -> (
    Env,
    P2PMarketplaceContractClient<'static>,
    Address,
    Address,
    token::Client<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = <Address as TestAddress>::generate(&env);
    let fee_collector = <Address as TestAddress>::generate(&env);

    // Setup the P2P marketplace contract
    let contract_id = env.register(P2PMarketplaceContract, ());
    let client = P2PMarketplaceContractClient::new(&env, &contract_id);

    // Setup the USDC token contract
    let (usdc_token_id, usdc_client) = create_token_contract(&env, &admin);

    // Initialize the P2P marketplace
    client.initialize(&admin, &usdc_token_id, &fee_collector);

    (env, client, admin, usdc_token_id, usdc_client, contract_id)
}

#[test]
fn test_initialize() {
    let (env, client, admin, usdc_token_id, _, contract_id) = setup_test_env();
    let (read_admin, read_usdc_token, _, _, _, _, _, _) = client.get_contract_info();

    assert_eq!(read_admin, admin);
    assert_eq!(read_usdc_token, usdc_token_id);
    assert_eq!(client.get_next_offer_id(), 0);
    assert_eq!(client.get_next_trade_id(), 0);
    assert_eq!(client.get_offers(), Map::new(&env));
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_initialize_already_initialized() {
    let (_, client, admin, usdc_token_id, _, _) = setup_test_env();
    let fee_collector = <Address as TestAddress>::generate(&client.env);
    client.initialize(&admin, &usdc_token_id, &fee_collector);
}

#[test]
fn test_create_offer() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000; // 100 USDC
    let kes_amount = 12_000_000_000; // 12,000 KES

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    assert_eq!(offer_id, 0);
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.seller, seller);
    assert_eq!(usdc_client.balance(&contract_id), usdc_amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // AlreadyHasActiveOffer
fn test_create_offer_already_has_active_offer() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount * 2, &contract_id);
    client.create_offer(&seller, &usdc_amount, &kes_amount);
    client.create_offer(&seller, &usdc_amount, &kes_amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")] // ContractPaused
fn test_create_offer_paused() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    client.pause();
    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    client.create_offer(&seller, &usdc_amount, &kes_amount);
}

#[test]
fn test_initiate_trade() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    assert_eq!(trade_id, 0);
    let trade = client.get_trade(&trade_id).unwrap();
    assert_eq!(trade.buyer, buyer);
    assert_eq!(trade.status, TradeStatus::Initiated);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")] // OfferNotFound
fn test_initiate_trade_offer_not_found() {
    let (env, client, admin, usdc_token_id, _, contract_id) = setup_test_env();
    let buyer = <Address as TestAddress>::generate(&env);
    client.initiate_trade(&buyer, &999);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // TradeAlreadyInitiated
fn test_initiate_trade_already_initiated() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    client.initiate_trade(&buyer, &offer_id);
    client.initiate_trade(&buyer, &offer_id);
}

#[test]
fn test_cancel_offer() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    assert_eq!(usdc_client.balance(&contract_id), usdc_amount);

    client.cancel_offer(&seller, &offer_id);
    assert_eq!(client.get_offers().len(), 0);
    assert_eq!(usdc_client.balance(&seller), usdc_amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // TradeAlreadyInitiated
fn test_cancel_offer_trade_already_initiated() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    client.initiate_trade(&buyer, &offer_id);
    client.cancel_offer(&seller, &offer_id);
}

#[test]
fn test_confirm_payment_and_release() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let fee_collector = client.get_fee_collector();
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    client.confirm_payment(&trade_id, &buyer);
    let trade = client.get_trade(&trade_id).unwrap();
    assert!(trade.buyer_confirmed_payment);

    client.confirm_payment(&trade_id, &seller);
    let trade = client.get_trade(&trade_id).unwrap();
    assert_eq!(trade.status, TradeStatus::Completed);

    let fee_rate = client.get_fee_rate();
    let fee = (usdc_amount * fee_rate as i128) / 10000;
    assert_eq!(usdc_client.balance(&buyer), usdc_amount - fee);
    assert_eq!(usdc_client.balance(&fee_collector), fee);
    assert_eq!(usdc_client.balance(&contract_id), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")] // TradeExpired
fn test_confirm_payment_trade_expired() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    let expiration = client.get_trade_expiration();
    env.ledger().set(LedgerInfo {
        timestamp: env.ledger().timestamp() + expiration + 1,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 10,
        max_entry_ttl: 50000,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 4096,
    });

    client.confirm_payment(&trade_id, &buyer);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")] // InvalidTradeStatus
fn test_confirm_payment_invalid_trade_status() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let usdc_amount = 100_000_000;
    let kes_amount = 12_000_000_000;

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    client.confirm_payment(&trade_id, &buyer);
    client.confirm_payment(&trade_id, &seller);
    client.confirm_payment(&trade_id, &buyer); // Already completed
}

// This is the key test that validates the bug fix.
#[test]
fn test_trade_completion_after_fix() {
    let (env, client, admin, usdc_token_id, usdc_client, contract_id) = setup_test_env();
    let seller = <Address as TestAddress>::generate(&env);
    let buyer = <Address as TestAddress>::generate(&env);
    let fee_collector = client.get_fee_collector();
    let usdc_amount = 500_000_000; // 500 USDC
    let kes_amount = 65_000_000_000; // 65,000 KES

    setup_token_balance(&env, &admin, &usdc_token_id, &seller, usdc_amount, &contract_id);

    // 1. Seller creates an offer
    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);
    assert_eq!(usdc_client.balance(&contract_id), usdc_amount);

    // 2. Buyer initiates a trade
    let trade_id = client.initiate_trade(&buyer, &offer_id);
    assert_eq!(client.get_trade(&trade_id).unwrap().status, TradeStatus::Initiated);

    // 3. Buyer confirms payment
    client.confirm_payment(&trade_id, &buyer);
    let trade = client.get_trade(&trade_id).unwrap();
    assert!(trade.buyer_confirmed_payment);
    assert!(!trade.seller_confirmed_payment);
    assert_eq!(trade.status, TradeStatus::Initiated);

    // 4. Seller confirms payment (this is where the original bug occurred)
    client.confirm_payment(&trade_id, &seller);

    // 5. Verify trade is now completed
    let trade = client.get_trade(&trade_id).unwrap();
    assert!(trade.seller_confirmed_payment);
    assert_eq!(trade.status, TradeStatus::Completed);

    // 6. Verify funds are released correctly
    let fee_rate = client.get_fee_rate();
    let fee_amount = (usdc_amount * fee_rate as i128) / 10000;
    let amount_to_buyer = usdc_amount - fee_amount;

    assert_eq!(usdc_client.balance(&buyer), amount_to_buyer);
    assert_eq!(usdc_client.balance(&fee_collector), fee_amount);
    assert_eq!(usdc_client.balance(&contract_id), 0);

    // 7. Verify offer is no longer active
    assert!(!client.get_active_offers().contains_key(seller));
}
