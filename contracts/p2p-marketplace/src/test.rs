#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_test_env() -> (Env, P2PMarketplaceContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, P2PMarketplaceContract);
    let client = P2PMarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::random(&env);
    let usdc_token_id = Address::random(&env);

    client.initialize(&admin, &usdc_token_id);

    (env, client, admin, usdc_token_id, contract_id)
}

#[test]
fn test_initialize() {
    let (env, client, admin, usdc_token_id, contract_id) = setup_test_env();

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_usdc_token_id(), usdc_token_id);
    assert_eq!(client.get_next_offer_id(), 0);
    assert_eq!(client.get_next_trade_id(), 0);
    assert_eq!(client.get_offers(), Map::new(&env));
    assert_eq!(client.get_trades(), Map::new(&env));
    assert_eq!(client.get_active_offers(), Map::new(&env));
    assert_eq!(client.is_paused(), false);
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_initialize_already_initialized() {
    let (env, client, admin, usdc_token_id, _) = setup_test_env();
    client.initialize(&admin, &usdc_token_id);
}

#[test]
fn test_create_offer() {
    let (env, client, _, usdc_token_id, contract_id) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    assert_eq!(offer_id, 0);
    assert_eq!(client.get_next_offer_id(), 1);

    let offers = client.get_offers();
    assert_eq!(offers.len(), 1);
    assert_eq!(offers.get(0).unwrap(), Offer {
        seller: seller.clone(),
        usdc_amount,
        kes_amount,
    });

    let active_offers = client.get_active_offers();
    assert_eq!(active_offers.len(), 1);
    assert_eq!(active_offers.get(seller).unwrap(), 0);

    // Check USDC transfer to contract
    assert_eq!(usdc_client.balance(&contract_id), usdc_amount);
}

#[test]
#[should_panic(expected = "AlreadyHasActiveOffer")]
fn test_create_offer_already_has_active_offer() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    client.create_offer(&seller, &usdc_amount, &kes_amount);
    client.create_offer(&seller, &usdc_amount, &kes_amount);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_create_offer_paused() {
    let (env, client, admin, usdc_token_id, _) = setup_test_env();
    client.pause(&admin);

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    client.create_offer(&seller, &usdc_amount, &kes_amount);
}

#[test]
fn test_initiate_trade() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    assert_eq!(trade_id, 0);
    assert_eq!(client.get_next_trade_id(), 1);

    let trades = client.get_trades();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades.get(0).unwrap(), Trade {
        offer_id,
        buyer: buyer.clone(),
        start_time: env.ledger().timestamp(),
        status: TradeStatus::Initiated,
        buyer_confirmed_payment: false,
        seller_confirmed_payment: false,
    });
}

#[test]
#[should_panic(expected = "OfferNotFound")]
fn test_initiate_trade_offer_not_found() {
    let (env, client, _, _, _) = setup_test_env();
    let buyer = Address::random(&env);
    client.initiate_trade(&buyer, &999);
}

#[test]
#[should_panic(expected = "TradeAlreadyInitiated")]
fn test_initiate_trade_already_initiated() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    client.initiate_trade(&buyer, &offer_id);
    client.initiate_trade(&buyer, &offer_id);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_initiate_trade_paused() {
    let (env, client, admin, usdc_token_id, _) = setup_test_env();
    client.pause(&admin);

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    client.initiate_trade(&buyer, &offer_id);
}

#[test]
fn test_cancel_offer() {
    let (env, client, _, usdc_token_id, contract_id) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    assert_eq!(usdc_client.balance(&contract_id), usdc_amount);

    client.cancel_offer(&seller, &offer_id);

    assert_eq!(client.get_offers().len(), 0);
    assert_eq!(client.get_active_offers().len(), 0);
    assert_eq!(usdc_client.balance(&contract_id), 0);
    assert_eq!(usdc_client.balance(&seller), usdc_amount);
}

#[test]
#[should_panic(expected = "OfferNotFound")]
fn test_cancel_offer_not_found() {
    let (env, client, _, _, _) = setup_test_env();
    let seller = Address::random(&env);
    client.cancel_offer(&seller, &999);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_cancel_offer_unauthorized() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let unauthorized_caller = Address::random(&env);
    client.cancel_offer(&unauthorized_caller, &offer_id);
}

#[test]
#[should_panic(expected = "TradeAlreadyInitiated")]
fn test_cancel_offer_trade_already_initiated() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    client.initiate_trade(&buyer, &offer_id);

    client.cancel_offer(&seller, &offer_id);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_cancel_offer_paused() {
    let (env, client, admin, usdc_token_id, _) = setup_test_env();
    client.pause(&admin);

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    client.cancel_offer(&seller, &offer_id);
}

#[test]
fn test_confirm_payment() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    // Buyer confirms payment
    client.confirm_payment(&trade_id, &buyer);
    let trade = client.get_trades().get(trade_id).unwrap();
    assert!(trade.buyer_confirmed_payment);
    assert!(!trade.seller_confirmed_payment);
    assert_eq!(trade.status, TradeStatus::Initiated);

    // Seller confirms payment
    client.confirm_payment(&trade_id, &seller);
    let trade = client.get_trades().get(trade_id).unwrap();
    assert!(trade.buyer_confirmed_payment);
    assert!(trade.seller_confirmed_payment);
    assert_eq!(trade.status, TradeStatus::Completed);

    // Check USDC transfer to buyer
    assert_eq!(usdc_client.balance(&buyer), usdc_amount);
}

#[test]
#[should_panic(expected = "TradeNotFound")]
fn test_confirm_payment_trade_not_found() {
    let (env, client, _, _, _) = setup_test_env();
    let buyer = Address::random(&env);
    client.confirm_payment(&999, &buyer);
}

#[test]
#[should_panic(expected = "TradeExpired")]
fn test_confirm_payment_trade_expired() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    env.ledger().set(env.ledger().timestamp() + 601);

    client.confirm_payment(&trade_id, &buyer);
}

#[test]
#[should_panic(expected = "InvalidTradeStatus")]
fn test_confirm_payment_invalid_trade_status() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    client.confirm_payment(&trade_id, &buyer);
    client.confirm_payment(&trade_id, &seller);

    client.confirm_payment(&trade_id, &buyer);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_confirm_payment_unauthorized() {
    let (env, client, _, usdc_token_id, _) = setup_test_env();

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    let unauthorized_caller = Address::random(&env);
    client.confirm_payment(&trade_id, &unauthorized_caller);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_confirm_payment_paused() {
    let (env, client, admin, usdc_token_id, _) = setup_test_env();
    client.pause(&admin);

    let seller = Address::random(&env);
    let usdc_amount = 1000;
    let kes_amount = 100000;

    let usdc_client = token::Client::new(&env, &usdc_token_id);
    usdc_client.mint(&seller, &usdc_amount);

    let offer_id = client.create_offer(&seller, &usdc_amount, &kes_amount);

    let buyer = Address::random(&env);
    let trade_id = client.initiate_trade(&buyer, &offer_id);

    client.confirm_payment(&trade_id, &buyer);
}
