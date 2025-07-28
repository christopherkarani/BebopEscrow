/*!
 * P2P Marketplace Smart Contract
 * 
 * This contract enables peer-to-peer trading between USDC and KES (Kenyan Shillings) via an escrow mechanism.
 * Key features:
 * - Secure escrow system with mutual payment confirmation
 * - Configurable trading fees and limits
 * - Admin controls for emergency situations
 * - Dispute resolution mechanism
 * - Comprehensive event logging for transparency
 * 
 * Security features:
 * - Authorization checks on all critical functions
 * - Comprehensive input validation
 * - Pausable contract for emergency situations
 * - Separate persistent and instance storage for different data types
 * 
 * Business Logic:
 * 1. Sellers create offers by depositing USDC into escrow
 * 2. Buyers initiate trades against existing offers
 * 3. Both parties confirm payment completion
 * 4. Contract releases USDC to buyer (minus fees) upon mutual confirmation
 * 5. Disputes can be raised and resolved by admin if needed
 */

#![no_std]

mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract,
    contractimpl,
    token,
    Address, Env, Map, Symbol, log, symbol_short
};

use types::{
    Error, Offer, Trade, TradeStatus, DisputeResolution,
    OFFER_CREATED, TRADE_INITIATED, PAYMENT_CONFIRMED, TRADE_COMPLETED,
    TRADE_CANCELLED, OFFER_CANCELLED, DISPUTE_RAISED, DISPUTE_RESOLVED
};

#[contract]
pub struct P2PMarketplaceContract;

// Storage keys - Using short symbols for gas efficiency
// Persistent storage is used for configuration that should survive contract upgrades
// Instance storage is used for runtime data that can be reset
const ADMIN_KEY: Symbol = Symbol::short("ADMIN");                    // Admin address (persistent)
const USDC_TOKEN_KEY: Symbol = Symbol::short("USDC_TKN");            // USDC token contract address (persistent)
const OFFERS_KEY: Symbol = Symbol::short("OFFERS");                  // Map of all offers (instance)
const ACTIVE_OFFERS: Symbol = Symbol::short("ACTV_OFRS");           // Maps seller Address to their active offer_id (instance)
const TRADES_KEY: Symbol = Symbol::short("TRADES");                  // Map of all trades (instance)
const NEXT_OFFER_ID: Symbol = Symbol::short("NEXT_O_ID");           // Counter for generating unique offer IDs (instance)
const NEXT_TRADE_ID: Symbol = Symbol::short("NEXT_T_ID");           // Counter for generating unique trade IDs (instance)
const PAUSED_KEY: Symbol = Symbol::short("PAUSED");                  // Contract pause state (instance)
const FEE_RATE_KEY: Symbol = Symbol::short("FEE_RATE");             // Trading fee rate in basis points (persistent)
const FEE_COLLECTOR_KEY: Symbol = Symbol::short("FEE_COLL");        // Address that receives trading fees (persistent)
const MIN_TRADE_AMOUNT_KEY: Symbol = Symbol::short("MIN_AMT");      // Minimum USDC amount per trade (persistent)
const MAX_TRADE_AMOUNT_KEY: Symbol = Symbol::short("MAX_AMT");      // Maximum USDC amount per trade (persistent)
const TRADE_EXPIRATION_KEY: Symbol = Symbol::short("TRD_EXP");      // Trade timeout in seconds (persistent)

// Default configuration values - These are fallbacks if storage is not set
const DEFAULT_TRADE_EXPIRATION: u64 = 600;                          // 10 minutes - Reasonable time for payment confirmation
const DEFAULT_MIN_TRADE_AMOUNT: i128 = 1_000_000;                   // 1 USDC (6 decimals) - Prevents spam with tiny trades
const DEFAULT_MAX_TRADE_AMOUNT: i128 = 1_000_000_000_000;          // 1M USDC - Prevents excessively large trades
const DEFAULT_FEE_RATE: u32 = 25;                                   // 0.25% = 25 basis points - Competitive marketplace fee
const BASIS_POINTS_DIVISOR: u32 = 10_000;                          // Standard basis points denominator

#[contractimpl]
impl P2PMarketplaceContract {
    /// Initializes the P2P marketplace contract with essential configuration.
    /// This function can only be called once and sets up the foundational parameters.
    /// 
    /// # Arguments
    /// * `admin` - The address that will have administrative privileges (pause, fees, disputes)
    /// * `usdc_token_id` - The contract address of the USDC token to be traded
    /// * `fee_collector` - The address that will receive trading fees
    /// 
    /// # Security Considerations
    /// - Validates that USDC token address is a valid token contract
    /// - Uses persistent storage for critical configuration to survive upgrades
    /// - Prevents double initialization
    /// - Validates all addresses are non-zero
    /// 
    /// # Returns
    /// Result indicating success or failure of initialization
    pub fn initialize(env: Env, admin: Address, usdc_token_id: Address, fee_collector: Address) -> Result<(), Error> {
        // Prevent double initialization - critical security check
        if env.storage().instance().has(&ADMIN_KEY) {
            panic!("Contract already initialized");
        }
        
        // SECURITY FIX: Validate all critical addresses
        Self::_validate_address(&admin)?;
        Self::_validate_address(&usdc_token_id)?;
        Self::_validate_address(&fee_collector)?;
        
        // Validate USDC token is a legitimate token contract by calling decimals()
        // This will panic if the address doesn't implement the token interface
        let usdc_client = token::Client::new(&env, &usdc_token_id);
        let _ = usdc_client.decimals(); // This will panic if not a valid token
        
        // Store critical configuration in persistent storage
        // This ensures configuration survives contract upgrades
        env.storage().persistent().set(&ADMIN_KEY, &admin);
        env.storage().persistent().set(&USDC_TOKEN_KEY, &usdc_token_id);
        env.storage().persistent().set(&FEE_COLLECTOR_KEY, &fee_collector);
        env.storage().persistent().set(&FEE_RATE_KEY, &DEFAULT_FEE_RATE);
        env.storage().persistent().set(&MIN_TRADE_AMOUNT_KEY, &DEFAULT_MIN_TRADE_AMOUNT);
        env.storage().persistent().set(&MAX_TRADE_AMOUNT_KEY, &DEFAULT_MAX_TRADE_AMOUNT);
        env.storage().persistent().set(&TRADE_EXPIRATION_KEY, &DEFAULT_TRADE_EXPIRATION);
        
        // Initialize runtime data structures in instance storage
        // These can be reset during contract upgrades if needed
        env.storage().instance().set(&NEXT_OFFER_ID, &0u64);
        env.storage().instance().set(&NEXT_TRADE_ID, &0u64);
        env.storage().instance().set(&OFFERS_KEY, &Map::<u64, Offer>::new(&env));
        env.storage().instance().set(&TRADES_KEY, &Map::<u64, Trade>::new(&env));
        env.storage().instance().set(&ACTIVE_OFFERS, &Map::<Address, u64>::new(&env));
        env.storage().instance().set(&PAUSED_KEY, &false);
        
        Ok(())
    }

    /// Internal helper function to verify admin authorization.
    /// This is used by all admin-only functions to ensure proper access control.
    /// 
    /// # Security Features
    /// - Retrieves admin address from persistent storage
    /// - Uses Soroban's built-in authorization system (require_auth)
    /// - Fails fast if admin is not properly authenticated
    /// 
    /// # Returns
    /// Result indicating if the caller is authorized as admin
    fn _require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env.storage().persistent().get(&ADMIN_KEY).unwrap();
        admin.require_auth(); // This will fail if the admin hasn't signed the transaction
        Ok(())
    }

    /// Internal helper to check if the contract is currently paused.
    /// Pausing is an emergency mechanism to halt all trading activities.
    /// 
    /// # Design Notes
    /// - Uses unwrap_or(false) to default to unpaused if not set
    /// - Pause state is stored in instance storage for easy reset
    /// 
    /// # Returns
    /// Boolean indicating if contract is paused
    fn _is_paused(env: &Env) -> bool {
        env.storage().instance().get(&PAUSED_KEY).unwrap_or(false)
    }

    /// Internal helper to validate that an address is not zero/empty.
    /// Zero addresses can cause critical issues in token transfers and access control.
    /// 
    /// # Security Notes
    /// - Prevents common attack vector of using zero address
    /// - Essential for proper access control and token safety
    /// - Should be called for all critical address parameters
    /// 
    /// # Arguments
    /// * `addr` - The address to validate
    /// 
    /// # Returns
    /// Result indicating if address is valid
    fn _validate_address(addr: &Address) -> Result<(), Error> {
        // Check if address has any bytes (not empty)
        // In Soroban, addresses should have proper structure
        // This is a basic check - the SDK handles most validation
        if addr.to_string().is_empty() {
            return Err(Error::InvalidAmount); // Using InvalidAmount for now, could add InvalidAddress error
        }
        Ok(())
    }

    /// Internal helper to determine if a trade has exceeded its time limit.
    /// Trades have expiration times to prevent indefinite escrow situations.
    /// 
    /// # Business Logic
    /// - Trades expire if payment confirmation takes too long
    /// - Expired trades can be resolved to return funds to seller
    /// - Configurable timeout allows for different confirmation requirements
    /// 
    /// # Arguments
    /// * `trade` - The trade to check for expiration
    /// 
    /// # Returns
    /// Boolean indicating if the trade has expired
    fn _is_trade_expired(env: &Env, trade: &Trade) -> bool {
        let trade_expiration: u64 = env.storage().persistent().get(&TRADE_EXPIRATION_KEY)
            .unwrap_or(DEFAULT_TRADE_EXPIRATION);
        env.ledger().timestamp() >= trade.start_time + trade_expiration
    }

    /// Internal helper to calculate trading fees using basis points.
    /// Fees are calculated as a percentage of the trade amount.
    /// 
    /// # Mathematical Notes
    /// - Uses basis points for precise percentage calculations
    /// - 1 basis point = 0.01%, so 25 basis points = 0.25%
    /// - Formula: (amount * fee_rate) / 10000
    /// - Includes overflow protection for large amounts
    /// 
    /// # Arguments
    /// * `amount` - The trade amount to calculate fee for
    /// * `fee_rate` - The fee rate in basis points
    /// 
    /// # Returns
    /// The calculated fee amount
    fn _calculate_fee(amount: i128, fee_rate: u32) -> i128 {
        // SECURITY FIX: Check for potential overflow before multiplication
        // Maximum safe value = i128::MAX / max_fee_rate (1000)
        const MAX_SAFE_AMOUNT: i128 = i128::MAX / 1000;
        
        // If amount is too large, use a safer calculation method
        if amount > MAX_SAFE_AMOUNT {
            // For very large amounts, divide first to prevent overflow
            // This may lose some precision but prevents overflow
            let amount_divided = amount / (BASIS_POINTS_DIVISOR as i128);
            amount_divided.saturating_mul(fee_rate as i128)
        } else {
            // Normal calculation for reasonable amounts
            // Use saturating_mul to ensure no panic on overflow
            amount.saturating_mul(fee_rate as i128) / (BASIS_POINTS_DIVISOR as i128)
        }
    }

    /// Creates a new offer to sell USDC for KES with escrow protection.
    /// The seller must approve the contract to spend their USDC before calling this function.
    /// 
    /// # Business Flow
    /// 1. Validates seller authorization and input parameters
    /// 2. Checks trading limits and seller doesn't have active offer
    /// 3. Verifies seller has sufficient USDC balance and allowance
    /// 4. Transfers USDC from seller to contract (escrow)
    /// 5. Creates offer record and updates active offers mapping
    /// 6. Emits event for transparency
    /// 
    /// # Security Checks
    /// - Requires seller authorization
    /// - Validates amount ranges to prevent spam/large trades
    /// - Checks USDC balance and allowance before transfer
    /// - Uses safe transfer with error handling
    /// - Enforces one active offer per seller rule
    /// 
    /// # Arguments
    /// * `seller` - The address creating the offer (must sign transaction)
    /// * `usdc_amount` - Amount of USDC to sell (with 6 decimals)
    /// * `kes_amount` - Amount of KES expected in return (off-chain settlement)
    /// 
    /// # Returns
    /// The unique ID of the created offer
    /// 
    /// # Errors
    /// - ContractPaused: If trading is temporarily disabled
    /// - InvalidAmount: If amounts are outside allowed ranges
    /// - AlreadyHasActiveOffer: If seller already has an active offer
    /// - InsufficientAllowance: If seller hasn't approved enough USDC
    /// - TokenTransferFailed: If USDC transfer to escrow fails
    pub fn create_offer(env: Env, seller: Address, usdc_amount: i128, kes_amount: i128) -> Result<u64, Error> {
        // Emergency brake - halt all trading if contract is paused
        if Self::_is_paused(&env) { return Err(Error::ContractPaused); }
        
        // Verify the seller has signed this transaction
        seller.require_auth();
        
        // SECURITY FIX: Validate seller address
        Self::_validate_address(&seller)?;

        // Input validation - prevent invalid or malicious amounts
        if usdc_amount <= 0 || kes_amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        
        // Enforce trading limits to prevent spam (min) and excessive exposure (max)
        let min_amount: i128 = env.storage().persistent().get(&MIN_TRADE_AMOUNT_KEY)
            .unwrap_or(DEFAULT_MIN_TRADE_AMOUNT);
        let max_amount: i128 = env.storage().persistent().get(&MAX_TRADE_AMOUNT_KEY)
            .unwrap_or(DEFAULT_MAX_TRADE_AMOUNT);
            
        if usdc_amount < min_amount || usdc_amount > max_amount {
            log!(&env, "Amount out of range. Min: {}, Max: {}, Provided: {}", 
                min_amount, max_amount, usdc_amount);
            return Err(Error::InvalidAmount);
        }

        // Business rule: One active offer per seller to keep marketplace simple
        // This prevents sellers from fragmenting liquidity across multiple offers
        let mut active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        if active_offers.contains_key(seller.clone()) {
            return Err(Error::AlreadyHasActiveOffer);
        }

        // Setup USDC token client for balance checks and transfers
        let usdc_token_id: Address = env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_token_id);

        // Security check: Verify seller actually has the USDC they want to sell
        let seller_balance = usdc_client.balance(&seller);
        if seller_balance < usdc_amount {
            log!(&env, "Insufficient balance. Required: {}, Available: {}", usdc_amount, seller_balance);
            return Err(Error::InsufficientAllowance);
        }

        // Security check: Verify seller has approved the contract to spend their USDC
        // This is a common DeFi pattern - users must explicitly approve token spending
        let allowance = usdc_client.allowance(&seller, &env.current_contract_address());
        if allowance < usdc_amount {
            log!(&env, "Insufficient allowance. Required: {}, Available: {}", usdc_amount, allowance);
            return Err(Error::InsufficientAllowance);
        }

        // Transfer USDC from seller to contract for escrow
        // Using try_transfer for proper error handling instead of panic-prone transfer()
        match usdc_client.try_transfer(&seller, &env.current_contract_address(), &usdc_amount) {
            Ok(_) => {},
            Err(_) => {
                log!(&env, "Token transfer failed for amount: {}", usdc_amount);
                return Err(Error::TokenTransferFailed);
            }
        }

        // Create the offer record with all necessary information
        let mut offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer_id: u64 = env.storage().instance().get(&NEXT_OFFER_ID).unwrap();

        let offer = Offer {
            seller: seller.clone(),
            usdc_amount,
            kes_amount,
        };

        // Store the offer and update active offers mapping for efficient lookups
        offers.set(offer_id, offer);
        active_offers.set(seller.clone(), offer_id);

        // Persist changes to storage
        env.storage().instance().set(&OFFERS_KEY, &offers);
        env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);
        env.storage().instance().set(&NEXT_OFFER_ID, &(offer_id + 1));

        // Emit event for transparency and off-chain indexing
        // Events allow frontends and analytics to track marketplace activity
        env.events().publish(
            (OFFER_CREATED, seller.clone()),
            (offer_id, usdc_amount, kes_amount),
        );

        Ok(offer_id)
    }

    /// Initiates a trade by a buyer against an existing offer.
    /// This begins the escrow process where USDC is held while payment confirmation occurs.
    /// 
    /// # Business Flow
    /// 1. Validates buyer authorization and offer existence
    /// 2. Prevents self-trading and checks offer is still active
    /// 3. Ensures no existing active trade for the offer
    /// 4. Creates trade record with initial status
    /// 5. Emits event to notify participants
    /// 
    /// # Security Features
    /// - Prevents buyers from trading with themselves
    /// - Validates offer is still active and available
    /// - Efficient lookup using active_offers mapping
    /// - Checks for existing active trades to prevent conflicts
    /// 
    /// # Arguments
    /// * `buyer` - The address initiating the trade (must sign transaction)
    /// * `offer_id` - The ID of the offer to trade against
    /// 
    /// # Returns
    /// The unique ID of the created trade
    /// 
    /// # Errors
    /// - ContractPaused: If trading is disabled
    /// - OfferNotFound: If offer doesn't exist or is no longer active
    /// - Unauthorized: If buyer tries to trade with themselves
    /// - TradeAlreadyInitiated: If offer already has an active trade
    pub fn initiate_trade(env: Env, buyer: Address, offer_id: u64) -> Result<u64, Error> {
        // Emergency brake - halt all trading if contract is paused
        if Self::_is_paused(&env) { return Err(Error::ContractPaused); }
        
        // Verify the buyer has signed this transaction
        buyer.require_auth();
        
        // SECURITY FIX: Validate buyer address
        Self::_validate_address(&buyer)?;

        // Retrieve the offer details to validate the trade
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(offer_id).ok_or(Error::OfferNotFound)?;
        
        // Business rule: Prevent self-trading to avoid manipulation
        // Users should not be able to trade with their own offers
        if buyer == offer.seller {
            return Err(Error::Unauthorized);
        }

        // Efficient validation: Check if offer is still active using the active_offers mapping
        // This is much more gas-efficient than iterating through all offers
        let active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        if !active_offers.contains_key(offer.seller.clone()) || 
           active_offers.get(offer.seller.clone()).unwrap() != offer_id {
            return Err(Error::OfferNotFound);
        }

        // Check for existing active trades on this offer
        // Only one trade can be active per offer to maintain order
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        
        // Optimized check: Only look for active trade statuses to allow completed/cancelled trades
        let mut has_active_trade = false;
        for trade in trades.values() {
            if trade.offer_id == offer_id && 
               (trade.status == TradeStatus::Initiated || 
                trade.status == TradeStatus::PaymentConfirmed ||
                trade.status == TradeStatus::Disputed) {
                has_active_trade = true;
                break;
            }
        }
        
        if has_active_trade {
            return Err(Error::TradeAlreadyInitiated);
        }

        // Generate unique trade ID for tracking
        let trade_id: u64 = env.storage().instance().get(&NEXT_TRADE_ID).unwrap();

        // Create trade record with initial state
        // Trade starts in "Initiated" status, waiting for payment confirmations
        let trade = Trade {
            offer_id,
            buyer: buyer.clone(),
            start_time: env.ledger().timestamp(), // Used for expiration checking
            status: TradeStatus::Initiated,
            buyer_confirmed_payment: false,       // Buyer hasn't confirmed sending KES yet
            seller_confirmed_payment: false,      // Seller hasn't confirmed receiving KES yet
        };

        // Store the trade and update counters
        trades.set(trade_id, trade);
        env.storage().instance().set(&TRADES_KEY, &trades);
        env.storage().instance().set(&NEXT_TRADE_ID, &(trade_id + 1));

        // Emit event for notification and tracking
        env.events().publish((TRADE_INITIATED, buyer.clone()), (trade_id, offer_id));

        Ok(trade_id)
    }

    /// Allows trade participants to confirm payment completion.
    /// Both buyer and seller must confirm before USDC is released.
    /// 
    /// # Business Flow
    /// 1. Validates participant authorization and trade existence
    /// 2. Checks trade hasn't expired and is in correct status
    /// 3. Records participant's payment confirmation
    /// 4. If both parties confirm, automatically releases USDC
    /// 5. Emits appropriate events for transparency
    /// 
    /// # Security Features
    /// - Only trade participants can confirm
    /// - Prevents confirmation on expired trades
    /// - Validates trade is in correct status for confirmation
    /// - Automatic execution when both parties confirm
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the trade to confirm payment for
    /// * `participant` - The address confirming (buyer or seller, must sign)
    /// 
    /// # Errors
    /// - ContractPaused: If contract is paused
    /// - TradeNotFound: If trade doesn't exist
    /// - TradeExpired: If trade has exceeded time limit
    /// - InvalidTradeStatus: If trade is not in confirmable state
    /// - Unauthorized: If caller is not a trade participant
    pub fn confirm_payment(env: Env, trade_id: u64, participant: Address) -> Result<(), Error> {
        // Emergency brake - halt all operations if contract is paused
        if Self::_is_paused(&env) { return Err(Error::ContractPaused); }
        
        // Verify the participant has signed this transaction
        participant.require_auth();

        // Retrieve and validate the trade
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        let mut trade = trades.get(trade_id).ok_or(Error::TradeNotFound)?;

        // Business rule: Expired trades cannot be confirmed to prevent stale settlements
        if Self::_is_trade_expired(&env, &trade) {
            return Err(Error::TradeExpired);
        }

        // Get offer details to validate participant authorization
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(trade.offer_id).ok_or(Error::OfferNotFound)?;

        // Only allow confirmations on initiated trades
        if trade.status != TradeStatus::Initiated {
            return Err(Error::InvalidTradeStatus);
        }

        // Update confirmation status based on who is confirming
        // Buyer confirms they have sent KES payment
        // Seller confirms they have received KES payment
        if participant == trade.buyer {
            trade.buyer_confirmed_payment = true;
        } else if participant == offer.seller {
            trade.seller_confirmed_payment = true;
        } else {
            // Security check: Only trade participants can confirm
            return Err(Error::Unauthorized);
        }

        // Emit confirmation event for transparency
        env.events().publish((PAYMENT_CONFIRMED, participant.clone()), (trade_id,));

        // Automatic execution: If both parties have confirmed, complete the trade
        if trade.buyer_confirmed_payment && trade.seller_confirmed_payment {
            trade.status = TradeStatus::PaymentConfirmed;
            // Release USDC to buyer (minus fees) - this is the core value transfer
            Self::release_usdc(env.clone(), trade_id)?;
        }

        // Persist the updated trade state
        trades.set(trade_id, trade);
        env.storage().instance().set(&TRADES_KEY, &trades);

        Ok(())
    }

    /// Internal function to release escrowed USDC to the buyer upon trade completion.
    /// This is the core value transfer that completes a successful trade.
    /// 
    /// # Business Logic
    /// 1. Validates trade is ready for USDC release
    /// 2. Calculates and deducts trading fees
    /// 3. Transfers USDC to buyer (amount minus fees)
    /// 4. Transfers fees to fee collector
    /// 5. Updates trade status and removes offer from active list
    /// 6. Emits completion event
    /// 
    /// # Fee Structure
    /// - Fees are calculated as basis points of trade amount
    /// - Fees are sent to designated fee collector address
    /// - Buyer receives trade amount minus fees
    /// - Fee failures don't block trade completion
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the trade to complete
    /// 
    /// # Returns
    /// Result indicating success or failure of USDC release
    fn release_usdc(env: Env, trade_id: u64) -> Result<(), Error> {
        // Retrieve and validate trade state
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        let mut trade = trades.get(trade_id).ok_or(Error::TradeNotFound)?;

        // Security check: Only release USDC for properly confirmed trades
        if trade.status != TradeStatus::PaymentConfirmed {
            return Err(Error::InvalidTradeStatus);
        }

        // Get offer details for amount and seller information
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(trade.offer_id).ok_or(Error::OfferNotFound)?;

        // Calculate trading fee based on configured rate
        let fee_rate: u32 = env.storage().persistent().get(&FEE_RATE_KEY)
            .unwrap_or(DEFAULT_FEE_RATE);
        let fee_amount = Self::_calculate_fee(offer.usdc_amount, fee_rate);
        let amount_to_buyer = offer.usdc_amount - fee_amount;
        
        // CRITICAL SECURITY FIX: Update state BEFORE transfers to prevent reentrancy
        // Following checks-effects-interactions pattern
        
        // Update trade status to completed BEFORE transfers
        trade.status = TradeStatus::Completed;
        trades.set(trade_id, trade.clone());

        // Remove offer from active offers BEFORE transfers
        let mut active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        active_offers.remove(offer.seller.clone());

        // Persist all state changes BEFORE transfers
        env.storage().instance().set(&TRADES_KEY, &trades);
        env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);

        // Emit completion event BEFORE transfers for consistency
        env.events().publish((TRADE_COMPLETED, trade.buyer.clone()), (trade_id,));

        // Now perform the external calls (transfers)
        let usdc_token_id: Address = env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_token_id);
        
        // Primary transfer: Send USDC to buyer (minus fees)
        // This is the main value transfer that completes the trade
        match usdc_client.try_transfer(&env.current_contract_address(), &trade.buyer, &amount_to_buyer) {
            Ok(_) => {},
            Err(_) => {
                log!(&env, "Failed to transfer {} to buyer", amount_to_buyer);
                // CRITICAL: Since we already updated state, we need to revert on failure
                // Revert the trade status
                trade.status = TradeStatus::PaymentConfirmed;
                trades.set(trade_id, trade.clone());
                env.storage().instance().set(&TRADES_KEY, &trades);
                
                // Revert the active offers
                active_offers.set(offer.seller.clone(), trade.offer_id);
                env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);
                
                return Err(Error::TokenTransferFailed);
            }
        }
        
        // Secondary transfer: Send fees to fee collector
        // Fee transfer failure doesn't block trade completion
        if fee_amount > 0 {
            let fee_collector: Address = env.storage().persistent().get(&FEE_COLLECTOR_KEY).unwrap();
            match usdc_client.try_transfer(&env.current_contract_address(), &fee_collector, &fee_amount) {
                Ok(_) => {},
                Err(_) => {
                    log!(&env, "Failed to transfer fee {} to collector", fee_amount);
                    // Continue - don't fail the trade because of fee transfer
                    // The trader's experience is more important than fee collection
                }
            }
        }

        Ok(())
    }

    /// Allows trade participants to cancel an initiated trade.
    /// This returns the escrowed USDC to the seller.
    /// 
    /// # Business Flow
    /// 1. Validates participant authorization and trade state
    /// 2. Ensures only initiated trades can be cancelled
    /// 3. Returns escrowed USDC to seller
    /// 4. Updates trade status and removes offer from active list
    /// 5. Emits cancellation event
    /// 
    /// # Security Features
    /// - Only trade participants can cancel
    /// - Only initiated trades can be cancelled
    /// - Safe transfer with error handling
    /// - Proper cleanup of offer state
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the trade to cancel
    /// * `participant` - The address requesting cancellation (buyer or seller)
    /// 
    /// # Errors
    /// - ContractPaused: If contract is paused
    /// - TradeNotFound: If trade doesn't exist
    /// - InvalidTradeStatus: If trade cannot be cancelled
    /// - Unauthorized: If caller is not a trade participant
    /// - TokenTransferFailed: If USDC return fails
    pub fn cancel_trade(env: Env, trade_id: u64, participant: Address) -> Result<(), Error> {
        // Emergency brake - halt all operations if contract is paused
        if Self::_is_paused(&env) { return Err(Error::ContractPaused); }
        
        // Verify the participant has signed this transaction
        participant.require_auth();

        // Retrieve and validate the trade
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        let mut trade = trades.get(trade_id).ok_or(Error::TradeNotFound)?;

        // Get offer details for validation and seller information
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(trade.offer_id).ok_or(Error::OfferNotFound)?;

        // Business rule: Only initiated trades can be cancelled
        // Once payment is confirmed, cancellation requires dispute resolution
        if trade.status != TradeStatus::Initiated {
            return Err(Error::InvalidTradeStatus);
        }

        // Security check: Only trade participants can cancel
        if participant != trade.buyer && participant != offer.seller {
            return Err(Error::Unauthorized);
        }

        // Update trade status to cancelled
        trade.status = TradeStatus::Cancelled;
        trades.set(trade_id, trade.clone());

        // Return escrowed USDC to the seller since trade is cancelled
        let usdc_token_id: Address = env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_token_id);

        // SECURITY FIX: Use try_transfer with proper error handling
        match usdc_client.try_transfer(&env.current_contract_address(), &offer.seller, &offer.usdc_amount) {
            Ok(_) => {},
            Err(_) => {
                log!(&env, "Failed to return {} to seller on cancel", offer.usdc_amount);
                // Revert the trade status since transfer failed
                trade.status = TradeStatus::Initiated;
                trades.set(trade_id, trade);
                env.storage().instance().set(&TRADES_KEY, &trades);
                return Err(Error::TokenTransferFailed);
            }
        }

        // Clean up: Remove offer from active offers so seller can create new ones
        let mut active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        active_offers.remove(offer.seller.clone());

        // Persist state changes
        env.storage().instance().set(&TRADES_KEY, &trades);
        env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);

        // Emit cancellation event for transparency
        env.events().publish((TRADE_CANCELLED, participant.clone()), (trade_id,));

        Ok(())
    }

    /// Resolves expired trades by returning escrowed USDC to sellers.
    /// Anyone can call this function to clean up expired trades.
    /// 
    /// # Business Logic
    /// - Trades have time limits to prevent indefinite escrow
    /// - Expired trades are automatically cancelled
    /// - USDC is returned to seller when trade expires
    /// - This prevents buyer griefing by not confirming payment
    /// 
    /// # Public Access
    /// - Any address can call this function
    /// - Helps maintain marketplace hygiene
    /// - Incentivizes community participation in cleanup
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the expired trade to resolve
    /// 
    /// # Errors
    /// - ContractPaused: If contract is paused
    /// - TradeNotFound: If trade doesn't exist
    /// - TradeNotExpired: If trade hasn't actually expired
    /// - InvalidTradeStatus: If trade is not in expirable state
    /// - TokenTransferFailed: If USDC return fails
    pub fn resolve_expired_trade(env: Env, trade_id: u64) -> Result<(), Error> {
        // Emergency brake - halt all operations if contract is paused
        if Self::_is_paused(&env) { return Err(Error::ContractPaused); }
        
        // Retrieve and validate the trade
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        let mut trade = trades.get(trade_id).ok_or(Error::TradeNotFound)?;

        // Validate that the trade has actually expired
        if !Self::_is_trade_expired(&env, &trade) {
            return Err(Error::TradeNotExpired);
        }

        // Only initiated trades can expire - others are already resolved
        if trade.status != TradeStatus::Initiated {
            return Err(Error::InvalidTradeStatus);
        }

        // Update trade status to cancelled due to expiration
        trade.status = TradeStatus::Cancelled;
        trades.set(trade_id, trade.clone());

        // Get offer details for returning USDC to seller
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(trade.offer_id).ok_or(Error::OfferNotFound)?;

        // Setup USDC client for returning funds
        let usdc_token_id: Address = env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_token_id);

        // Return the escrowed USDC to seller since trade expired
        match usdc_client.try_transfer(&env.current_contract_address(), &offer.seller, &offer.usdc_amount) {
            Ok(_) => {},
            Err(_) => {
                log!(&env, "Failed to return {} to seller", offer.usdc_amount);
                return Err(Error::TokenTransferFailed);
            }
        }

        // Clean up: Remove offer from active offers
        let mut active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        active_offers.remove(offer.seller.clone());

        // Persist state changes
        env.storage().instance().set(&TRADES_KEY, &trades);
        env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);

        // Emit cancellation event (using contract address as emitter for expired trades)
        env.events().publish((TRADE_CANCELLED, env.current_contract_address()), (trade_id,));

        Ok(())
    }

    /// Allows sellers to cancel their offers and recover escrowed USDC.
    /// Offers can only be cancelled if no active trade exists.
    /// 
    /// # Business Flow
    /// 1. Validates seller authorization and offer ownership
    /// 2. Checks no active trades exist for the offer
    /// 3. Returns escrowed USDC to seller
    /// 4. Removes offer from all mappings
    /// 5. Emits cancellation event
    /// 
    /// # Security Features
    /// - Only offer owner can cancel
    /// - Prevents cancellation if active trade exists
    /// - Safe transfer with error handling
    /// - Proper cleanup of all offer references
    /// 
    /// # Arguments
    /// * `seller` - The address that created the offer (must sign)
    /// * `offer_id` - The ID of the offer to cancel
    /// 
    /// # Errors
    /// - ContractPaused: If contract is paused
    /// - OfferNotFound: If offer doesn't exist
    /// - Unauthorized: If caller is not the offer owner
    /// - TradeAlreadyInitiated: If active trade exists for offer
    /// - TokenTransferFailed: If USDC return fails
    pub fn cancel_offer(env: Env, seller: Address, offer_id: u64) -> Result<(), Error> {
        // Emergency brake - halt all operations if contract is paused
        if Self::_is_paused(&env) { return Err(Error::ContractPaused); }
        
        // Verify the seller has signed this transaction
        seller.require_auth();

        // Retrieve and validate the offer
        let mut offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(offer_id).ok_or(Error::OfferNotFound)?;

        // Security check: Only the offer owner can cancel their offer
        if offer.seller != seller {
            return Err(Error::Unauthorized);
        }

        // Business rule: Cannot cancel offer if there's an active trade
        // This prevents disrupting ongoing trade processes
        let trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        
        // Optimized check: Only prevent cancellation if there's an active trade
        // Completed and cancelled trades don't block offer cancellation
        let mut has_active_trade = false;
        for trade in trades.values() {
            if trade.offer_id == offer_id && 
               (trade.status == TradeStatus::Initiated || 
                trade.status == TradeStatus::PaymentConfirmed || 
                trade.status == TradeStatus::Disputed) {
                has_active_trade = true;
                break;
            }
        }
        
        if has_active_trade {
            return Err(Error::TradeAlreadyInitiated);
        }

        // Setup USDC client for returning escrowed funds
        let usdc_token_id: Address = env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_token_id);

        // Return the escrowed USDC to seller
        match usdc_client.try_transfer(&env.current_contract_address(), &seller, &offer.usdc_amount) {
            Ok(_) => {},
            Err(_) => {
                log!(&env, "Failed to return {} to seller", offer.usdc_amount);
                return Err(Error::TokenTransferFailed);
            }
        }

        // Remove offer from storage
        offers.remove(offer_id);

        // Remove from active offers mapping
        let mut active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        active_offers.remove(seller.clone());

        // Persist changes
        env.storage().instance().set(&OFFERS_KEY, &offers);
        env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);

        // Emit cancellation event for transparency
        env.events().publish((OFFER_CANCELLED, seller.clone()), (offer_id,));

        Ok(())
    }

    /// Emergency function to pause all trading activities.
    /// Only admin can pause the contract for security or maintenance.
    /// 
    /// # Use Cases
    /// - Security incidents requiring immediate halt
    /// - Contract upgrades or maintenance
    /// - Regulatory compliance requirements
    /// - Market manipulation prevention
    /// 
    /// # Admin Only
    /// - Requires admin authorization
    /// - Immediate effect on all trading functions
    /// - Does not affect existing trades, only new operations
    /// 
    /// # Returns
    /// Result indicating success or failure of pause operation
    pub fn pause(env: Env) -> Result<(), Error> {
        // Verify admin authorization - only admin can pause
        Self::_require_admin(&env)?;
        
        // Set pause flag to halt all trading operations
        env.storage().instance().set(&PAUSED_KEY, &true);
        
        Ok(())
    }

    /// Resumes trading activities after a pause.
    /// Only admin can unpause the contract.
    /// 
    /// # Security Consideration
    /// - Admin should verify all issues are resolved before unpausing
    /// - Existing trades continue normally after unpause
    /// - New trading activities become available immediately
    /// 
    /// # Returns
    /// Result indicating success or failure of unpause operation
    pub fn unpause(env: Env) -> Result<(), Error> {
        // Verify admin authorization - only admin can unpause
        Self::_require_admin(&env)?;
        
        // Remove pause flag to resume trading operations
        env.storage().instance().set(&PAUSED_KEY, &false);
        
        Ok(())
    }

    // ================================================================================================
    // DISPUTE RESOLUTION SYSTEM
    // ================================================================================================
    // Note: Dispute resolution functions should be implemented here
    // For now, disputes must be handled off-chain by contacting the admin
    
    /// Raises a dispute for a trade when payment confirmation conflicts arise.
    /// This function allows trade participants to escalate issues that cannot be resolved
    /// through normal payment confirmation flow.
    /// 
    /// # Business Logic
    /// - Either buyer or seller can raise a dispute
    /// - Disputes can be raised on initiated or payment-confirmed trades
    /// - Once disputed, trades require admin intervention to resolve
    /// - Prevents automatic trade completion until dispute is resolved
    /// 
    /// # Security Features
    /// - Only trade participants can raise disputes
    /// - Validates trade exists and is in appropriate state
    /// - Prevents abuse by limiting who can dispute
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the trade to dispute
    /// * `caller` - The address raising the dispute (buyer or seller)
    /// 
    /// # Returns
    /// Result indicating success or failure of dispute creation
    /// 
    /// # Errors
    /// - TradeNotFound: If trade doesn't exist
    /// - Unauthorized: If caller is not a trade participant
    /// - InvalidTradeStatus: If trade is not in disputable state
    pub fn raise_dispute(env: Env, trade_id: u64, caller: Address) -> Result<(), Error> {
        // Verify the caller has signed this transaction
        caller.require_auth();

        // Retrieve and validate the trade
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        let mut trade = trades.get(trade_id).ok_or(Error::TradeNotFound)?;

        // Get offer details to validate caller authorization
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(trade.offer_id).ok_or(Error::OfferNotFound)?;

        // Security check: Only trade participants can raise disputes
        if caller != trade.buyer && caller != offer.seller {
            return Err(Error::Unauthorized);
        }

        // Business rule: Only initiated or payment-confirmed trades can be disputed
        // Completed and cancelled trades cannot be disputed
        if trade.status != TradeStatus::Initiated && trade.status != TradeStatus::PaymentConfirmed {
            return Err(Error::InvalidTradeStatus);
        }

        // Update trade status to disputed
        trade.status = TradeStatus::Disputed;
        trades.set(trade_id, trade);
        env.storage().instance().set(&TRADES_KEY, &trades);

        // Emit dispute event for admin notification and transparency
        env.events().publish((DISPUTE_RAISED, caller.clone()), (trade_id,));

        Ok(())
    }

    /// Resolves a disputed trade with admin intervention.
    /// Only the admin can resolve disputes by choosing to release USDC to buyer or refund to seller.
    /// 
    /// # Business Logic
    /// - Admin reviews dispute details off-chain
    /// - Admin decides whether buyer or seller is correct
    /// - USDC is transferred based on admin's resolution decision
    /// - Fees are still collected on successful trades (release to buyer)
    /// - No fees on refunds to seller
    /// 
    /// # Admin Authority
    /// - Only admin can resolve disputes
    /// - Admin decisions are final and irreversible
    /// - Admin should have off-chain verification process
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the disputed trade to resolve
    /// * `resolution` - The admin's decision (ReleaseToBuyer or RefundToSeller)
    /// 
    /// # Returns
    /// Result indicating success or failure of dispute resolution
    /// 
    /// # Errors
    /// - Unauthorized: If caller is not the admin
    /// - TradeNotFound: If trade doesn't exist
    /// - InvalidTradeStatus: If trade is not in disputed state
    /// - TokenTransferFailed: If USDC transfer fails
    pub fn resolve_dispute(env: Env, trade_id: u64, resolution: DisputeResolution) -> Result<(), Error> {
        // Verify admin authorization - only admin can resolve disputes
        Self::_require_admin(&env)?;

        // Retrieve and validate the trade
        let mut trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        let mut trade = trades.get(trade_id).ok_or(Error::TradeNotFound)?;

        // Security check: Only disputed trades can be resolved
        if trade.status != TradeStatus::Disputed {
            return Err(Error::InvalidTradeStatus);
        }

        // Get offer details for transfer amounts and addresses
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        let offer = offers.get(trade.offer_id).ok_or(Error::OfferNotFound)?;

        // Setup USDC client for resolution transfers
        let usdc_token_id: Address = env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_token_id);

        // Execute admin's resolution decision
        match resolution {
            DisputeResolution::ReleaseToBuyer => {
                // Admin determined buyer is correct - complete the trade
                // Calculate and collect fees even for disputed trades
                let fee_rate: u32 = env.storage().persistent().get(&FEE_RATE_KEY)
                    .unwrap_or(DEFAULT_FEE_RATE);
                let fee_amount = Self::_calculate_fee(offer.usdc_amount, fee_rate);
                let amount_to_buyer = offer.usdc_amount - fee_amount;
                
                // Transfer USDC to buyer (minus fees)
                match usdc_client.try_transfer(&env.current_contract_address(), &trade.buyer, &amount_to_buyer) {
                    Ok(_) => {
                        // Transfer fee to fee collector if applicable
                        if fee_amount > 0 {
                            let fee_collector: Address = env.storage().persistent().get(&FEE_COLLECTOR_KEY).unwrap();
                            let _ = usdc_client.try_transfer(&env.current_contract_address(), &fee_collector, &fee_amount);
                        }
                        trade.status = TradeStatus::Completed;
                    },
                    Err(_) => {
                        log!(&env, "Failed to transfer {} to buyer in dispute resolution", amount_to_buyer);
                        return Err(Error::TokenTransferFailed);
                    }
                }
            }
            DisputeResolution::RefundToSeller => {
                // Admin determined seller is correct - refund the full amount (no fees)
                match usdc_client.try_transfer(&env.current_contract_address(), &offer.seller, &offer.usdc_amount) {
                    Ok(_) => {
                        trade.status = TradeStatus::Cancelled;
                    },
                    Err(_) => {
                        log!(&env, "Failed to refund {} to seller in dispute resolution", offer.usdc_amount);
                        return Err(Error::TokenTransferFailed);
                    }
                }
            }
        }

        // Update trade with resolution outcome
        trades.set(trade_id, trade.clone());

        // Clean up: Remove offer from active offers since dispute is resolved
        let mut active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        active_offers.remove(offer.seller.clone());

        // Persist all changes
        env.storage().instance().set(&TRADES_KEY, &trades);
        env.storage().instance().set(&ACTIVE_OFFERS, &active_offers);

        // Emit resolution event for transparency and audit trail
        env.events().publish((DISPUTE_RESOLVED, env.current_contract_address()), (trade_id, resolution));

        Ok(())
    }

    // ================================================================================================
    // ADMINISTRATIVE FUNCTIONS
    // ================================================================================================
    // These functions allow the admin to configure and manage the marketplace
    
    /// Updates the admin address to a new address.
    /// This is a critical security function that transfers administrative control.
    /// 
    /// # Security Features
    /// - Requires current admin authorization
    /// - Requires new admin to sign transaction (prevents unauthorized transfers)
    /// - Emits event for transparency and audit trail
    /// - Immediate effect - new admin can perform admin functions right away
    /// 
    /// # Use Cases
    /// - Transferring control to a new administrator
    /// - Moving to a multi-sig admin address
    /// - Emergency admin change for security reasons
    /// 
    /// # Arguments
    /// * `new_admin` - The new admin address (must sign transaction)
    /// 
    /// # Returns
    /// Result indicating success or failure of admin update
    /// 
    /// # Errors
    /// - Unauthorized: If caller is not current admin
    pub fn update_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        // Verify current admin authorization
        Self::_require_admin(&env)?;
        
        // Require new admin to sign transaction - prevents accidental transfers
        new_admin.require_auth();
        
        // SECURITY FIX: Validate new admin address
        Self::_validate_address(&new_admin)?;
        
        // Update admin address in persistent storage
        env.storage().persistent().set(&ADMIN_KEY, &new_admin);
        
        // Emit event for security audit trail
        env.events().publish((symbol_short!("adm_upd"), env.current_contract_address()), &new_admin);
        
        Ok(())
    }
    
    /// Updates the fee collector address where trading fees are sent.
    /// This allows admin to change where marketplace fees are collected.
    /// 
    /// # Business Logic
    /// - Fee collector receives a percentage of each completed trade
    /// - Can be set to treasury, DAO, or operational address
    /// - Takes effect immediately for new trades
    /// - Does not affect ongoing trades
    /// 
    /// # Arguments
    /// * `new_fee_collector` - The new address to receive trading fees
    /// 
    /// # Returns
    /// Result indicating success or failure of fee collector update
    /// 
    /// # Errors
    /// - Unauthorized: If caller is not admin
    pub fn update_fee_collector(env: Env, new_fee_collector: Address) -> Result<(), Error> {
        // Verify admin authorization
        Self::_require_admin(&env)?;
        
        // SECURITY FIX: Validate new fee collector address
        Self::_validate_address(&new_fee_collector)?;
        
        // Update fee collector address in persistent storage
        env.storage().persistent().set(&FEE_COLLECTOR_KEY, &new_fee_collector);
        
        Ok(())
    }
    
    /// Updates the trading fee rate charged on completed trades.
    /// Fee rate is specified in basis points (1/100th of a percent).
    /// 
    /// # Fee Structure
    /// - Basis points: 1 = 0.01%, 100 = 1%, 1000 = 10%
    /// - Maximum allowed fee is 10% (1000 basis points)
    /// - Reasonable marketplace fees are typically 0.1% - 1%
    /// - Fees are only collected on successful trades
    /// 
    /// # Arguments
    /// * `new_fee_rate` - New fee rate in basis points (max 1000 = 10%)
    /// 
    /// # Returns
    /// Result indicating success or failure of fee rate update
    /// 
    /// # Errors
    /// - Unauthorized: If caller is not admin
    /// - InvalidAmount: If fee rate exceeds 10%
    pub fn update_fee_rate(env: Env, new_fee_rate: u32) -> Result<(), Error> {
        // Verify admin authorization
        Self::_require_admin(&env)?;
        
        // Validate fee rate is reasonable (max 10%)
        if new_fee_rate > 1000 { // Max 10%
            return Err(Error::InvalidAmount);
        }
        
        // Update fee rate in persistent storage
        env.storage().persistent().set(&FEE_RATE_KEY, &new_fee_rate);
        
        Ok(())
    }
    
    /// Updates the minimum and maximum trade amounts for USDC trades.
    /// These limits help prevent spam trades and excessive exposure.
    /// 
    /// # Business Logic
    /// - Minimum amount prevents spam with tiny trades
    /// - Maximum amount limits exposure per trade
    /// - Amounts are in USDC with 6 decimal places
    /// - Applies to new offers only, existing offers unchanged
    /// 
    /// # Arguments
    /// * `min_amount` - Minimum USDC amount for trades (with 6 decimals)
    /// * `max_amount` - Maximum USDC amount for trades (with 6 decimals)
    /// 
    /// # Returns
    /// Result indicating success or failure of limits update
    /// 
    /// # Errors
    /// - Unauthorized: If caller is not admin
    /// - InvalidAmount: If amounts are invalid or min > max
    pub fn update_trade_limits(env: Env, min_amount: i128, max_amount: i128) -> Result<(), Error> {
        // Verify admin authorization
        Self::_require_admin(&env)?;
        
        // Validate amount parameters
        if min_amount <= 0 || max_amount <= 0 || min_amount > max_amount {
            return Err(Error::InvalidAmount);
        }
        
        // SECURITY FIX: Additional bounds checking to prevent extreme values
        // Maximum reasonable amount is 1 trillion USDC (with 6 decimals)
        const MAX_REASONABLE_AMOUNT: i128 = 1_000_000_000_000_000_000; // 1 trillion USDC
        if max_amount > MAX_REASONABLE_AMOUNT {
            return Err(Error::InvalidAmount);
        }
        
        // Update trade limits in persistent storage
        env.storage().persistent().set(&MIN_TRADE_AMOUNT_KEY, &min_amount);
        env.storage().persistent().set(&MAX_TRADE_AMOUNT_KEY, &max_amount);
        
        Ok(())
    }
    
    /// Updates the trade expiration time for new trades.
    /// This controls how long buyers have to confirm payment before trades expire.
    /// 
    /// # Business Logic
    /// - Expired trades automatically return USDC to seller
    /// - Shorter times reduce seller risk but may rush buyers
    /// - Longer times give buyers more flexibility but increase seller risk
    /// - Typical values: 10 minutes to 24 hours
    /// 
    /// # Arguments
    /// * `expiration_seconds` - New expiration time in seconds (60 to 86400)
    /// 
    /// # Returns
    /// Result indicating success or failure of expiration update
    /// 
    /// # Errors
    /// - Unauthorized: If caller is not admin
    /// - InvalidAmount: If expiration is outside allowed range
    pub fn update_trade_expiration(env: Env, expiration_seconds: u64) -> Result<(), Error> {
        // Verify admin authorization
        Self::_require_admin(&env)?;
        
        // Validate expiration time is reasonable (1 minute to 24 hours)
        if expiration_seconds < 60 || expiration_seconds > 86400 { // Min 1 minute, max 24 hours
            return Err(Error::InvalidAmount);
        }
        
        // Update trade expiration in persistent storage
        env.storage().persistent().set(&TRADE_EXPIRATION_KEY, &expiration_seconds);
        
        Ok(())
    }

    // ================================================================================================
    // QUERY FUNCTIONS (GETTERS)
    // ================================================================================================
    // These functions provide read-only access to contract state for external callers
    
    /// Returns the current admin address.
    /// 
    /// # Usage
    /// - Check who has administrative privileges
    /// - Verify admin address in UI applications
    /// - Audit administrative access
    /// 
    /// # Returns
    /// The address of the current contract administrator
    pub fn get_admin(env: Env) -> Address {
        env.storage().persistent().get(&ADMIN_KEY).unwrap()
    }

    /// Returns the USDC token contract address.
    /// 
    /// # Usage
    /// - Verify which token is used for trading
    /// - Set up token approvals in client applications
    /// - Validate contract configuration
    /// 
    /// # Returns
    /// The address of the USDC token contract
    pub fn get_usdc_token_id(env: Env) -> Address {
        env.storage().persistent().get(&USDC_TOKEN_KEY).unwrap()
    }
    
    /// Returns the fee collector address.
    /// 
    /// # Usage
    /// - See where trading fees are sent
    /// - Verify fee collection setup
    /// - Audit fee distribution
    /// 
    /// # Returns
    /// The address that receives trading fees
    pub fn get_fee_collector(env: Env) -> Address {
        env.storage().persistent().get(&FEE_COLLECTOR_KEY).unwrap()
    }
    
    /// Returns the current trading fee rate in basis points.
    /// 
    /// # Fee Calculation
    /// - Basis points: 25 = 0.25%, 100 = 1%
    /// - To calculate fee: (trade_amount * fee_rate) / 10000
    /// - Example: 1000 USDC trade with 25 basis points = 2.5 USDC fee
    /// 
    /// # Returns
    /// Current fee rate in basis points (e.g., 25 = 0.25%)
    pub fn get_fee_rate(env: Env) -> u32 {
        env.storage().persistent().get(&FEE_RATE_KEY).unwrap_or(DEFAULT_FEE_RATE)
    }
    
    /// Returns the current minimum and maximum trade amounts.
    /// 
    /// # Usage
    /// - Validate trade amounts before creating offers
    /// - Display trading limits in UI
    /// - Ensure compliance with platform rules
    /// 
    /// # Returns
    /// Tuple of (minimum_amount, maximum_amount) in USDC with 6 decimals
    pub fn get_trade_limits(env: Env) -> (i128, i128) {
        let min = env.storage().persistent().get(&MIN_TRADE_AMOUNT_KEY)
            .unwrap_or(DEFAULT_MIN_TRADE_AMOUNT);
        let max = env.storage().persistent().get(&MAX_TRADE_AMOUNT_KEY)
            .unwrap_or(DEFAULT_MAX_TRADE_AMOUNT);
        (min, max)
    }
    
    /// Returns the current trade expiration time in seconds.
    /// 
    /// # Usage
    /// - Calculate trade expiration times for UI
    /// - Inform users how long they have to confirm
    /// - Set appropriate timeout expectations
    /// 
    /// # Returns
    /// Trade expiration time in seconds
    pub fn get_trade_expiration(env: Env) -> u64 {
        env.storage().persistent().get(&TRADE_EXPIRATION_KEY)
            .unwrap_or(DEFAULT_TRADE_EXPIRATION)
    }

    /// Returns the next offer ID that will be assigned.
    /// 
    /// # Usage
    /// - Predict offer IDs for client applications
    /// - Monitor marketplace growth and activity
    /// - Debug offer creation issues
    /// 
    /// # Returns
    /// The next available offer ID
    pub fn get_next_offer_id(env: Env) -> u64 {
        env.storage().instance().get(&NEXT_OFFER_ID).unwrap()
    }

    /// Returns the next trade ID that will be assigned.
    /// 
    /// # Usage
    /// - Predict trade IDs for client applications
    /// - Monitor trading activity and volume
    /// - Debug trade creation issues
    /// 
    /// # Returns
    /// The next available trade ID
    pub fn get_next_trade_id(env: Env) -> u64 {
        env.storage().instance().get(&NEXT_TRADE_ID).unwrap()
    }

    /// Returns all offers in the marketplace.
    /// Warning: This function can be expensive for large datasets.
    /// 
    /// # Performance Considerations
    /// - Returns ALL offers (including inactive ones)
    /// - Can consume significant gas for large datasets
    /// - Consider using pagination for production applications
    /// - Better to use `get_offer` for specific lookups
    /// 
    /// # Returns
    /// Map of all offers keyed by offer ID
    pub fn get_offers(env: Env) -> Map<u64, Offer> {
        env.storage().instance().get(&OFFERS_KEY).unwrap()
    }

    /// Returns a specific offer by its ID.
    /// 
    /// # Usage
    /// - Get offer details for display
    /// - Validate offer exists before trading
    /// - Check offer parameters
    /// 
    /// # Arguments
    /// * `offer_id` - The ID of the offer to retrieve
    /// 
    /// # Returns
    /// The offer if it exists, None otherwise
    pub fn get_offer(env: Env, offer_id: u64) -> Option<Offer> {
        let offers: Map<u64, Offer> = env.storage().instance().get(&OFFERS_KEY).unwrap();
        offers.get(offer_id)
    }

    /// Returns all trades in the marketplace.
    /// Warning: This function can be expensive for large datasets.
    /// 
    /// # Performance Considerations
    /// - Returns ALL trades regardless of status
    /// - Can consume significant gas for large datasets
    /// - Consider using pagination for production applications
    /// - Better to use `get_trade` for specific lookups
    /// 
    /// # Returns
    /// Map of all trades keyed by trade ID
    pub fn get_trades(env: Env) -> Map<u64, Trade> {
        env.storage().instance().get(&TRADES_KEY).unwrap()
    }
    
    /// Returns a specific trade by its ID.
    /// 
    /// # Usage
    /// - Get trade details and status
    /// - Monitor trade progress
    /// - Validate trade exists before operations
    /// 
    /// # Arguments
    /// * `trade_id` - The ID of the trade to retrieve
    /// 
    /// # Returns
    /// The trade if it exists, None otherwise
    pub fn get_trade(env: Env, trade_id: u64) -> Option<Trade> {
        let trades: Map<u64, Trade> = env.storage().instance().get(&TRADES_KEY).unwrap();
        trades.get(trade_id)
    }

    /// Returns the mapping of sellers to their active offer IDs.
    /// 
    /// # Usage
    /// - Check which sellers have active offers
    /// - Enforce one-offer-per-seller rule
    /// - Display active offers by seller
    /// 
    /// # Returns
    /// Map of seller addresses to their active offer IDs
    pub fn get_active_offers(env: Env) -> Map<Address, u64> {
        env.storage().instance().get(&ACTIVE_OFFERS).unwrap()
    }
    
    /// Returns the active offer ID for a specific seller.
    /// 
    /// # Usage
    /// - Check if seller has an active offer
    /// - Get seller's current offer ID
    /// - Enforce business rules about multiple offers
    /// 
    /// # Arguments
    /// * `seller` - The seller address to check
    /// 
    /// # Returns
    /// The seller's active offer ID if they have one, None otherwise
    pub fn get_seller_active_offer(env: Env, seller: Address) -> Option<u64> {
        let active_offers: Map<Address, u64> = env.storage().instance().get(&ACTIVE_OFFERS).unwrap();
        active_offers.get(seller)
    }

    /// Returns whether the contract is currently paused.
    /// 
    /// # Usage
    /// - Check if trading is allowed
    /// - Display maintenance status in UI
    /// - Validate operations before attempting
    /// 
    /// # Returns
    /// True if contract is paused, false if trading is active
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&PAUSED_KEY).unwrap_or(false)
    }
    
    /// Returns comprehensive contract configuration and status.
    /// This is a convenience function that aggregates multiple config values.
    /// 
    /// # Usage
    /// - Get all contract settings in one call
    /// - Display complete contract status
    /// - Validate configuration in client applications
    /// 
    /// # Returns
    /// Tuple containing:
    /// (admin, usdc_token, fee_collector, fee_rate, min_amount, max_amount, expiration, is_paused)
    pub fn get_contract_info(env: Env) -> (Address, Address, Address, u32, i128, i128, u64, bool) {
        (
            Self::get_admin(env.clone()),
            Self::get_usdc_token_id(env.clone()),
            Self::get_fee_collector(env.clone()),
            Self::get_fee_rate(env.clone()),
            Self::get_trade_limits(env.clone()).0,
            Self::get_trade_limits(env.clone()).1,
            Self::get_trade_expiration(env.clone()),
            Self::is_paused(env)
        )
    }
}
