/*!
 * Type Definitions for P2P Marketplace Smart Contract
 * 
 * This module defines all the data structures, enums, and constants used throughout
 * the P2P marketplace contract. Each type is carefully designed to represent specific
 * aspects of the trading system with clear semantics and efficient storage.
 */

use soroban_sdk::{contracterror, contracttype, Address, Symbol};

// ================================================================================================
// CORE DATA STRUCTURES
// ================================================================================================

/// Represents a sell offer in the marketplace.
/// 
/// An offer is created when a seller wants to exchange USDC for KES. The seller deposits
/// USDC into the contract as escrow, and the offer becomes available for buyers to trade against.
/// 
/// # Design Decisions
/// - Seller address identifies who created the offer and owns the escrowed USDC
/// - USDC amount is stored with 6 decimal precision (Stellar USDC standard)
/// - KES amount represents the off-chain currency amount expected in return
/// - No expiration field yet - could be added in future versions
/// - No partial fulfillment support - offers are atomic (all-or-nothing)
/// 
/// # Business Logic
/// - One offer per seller (enforced by contract logic)
/// - USDC is held in escrow until trade completion or offer cancellation
/// - Exchange rate is implicitly defined by usdc_amount / kes_amount ratio
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Offer {
    /// The address of the seller who created this offer
    /// This address owns the escrowed USDC and will receive KES payment off-chain
    pub seller: Address,
    
    /// Amount of USDC being offered for sale (with 6 decimal places)
    /// This amount is held in escrow by the contract until trade completion
    /// Example: 1_000_000 = 1 USDC, 500_000 = 0.5 USDC
    pub usdc_amount: i128,
    
    /// Amount of KES (Kenyan Shillings) expected in return
    /// This is settled off-chain through traditional payment methods
    /// The ratio usdc_amount/kes_amount defines the exchange rate
    /// Example: 150_000 = 150 KES (assuming 3 decimal precision)
    pub kes_amount: i128,
}

/// Represents an active trade between a buyer and seller.
/// 
/// A trade is initiated when a buyer chooses to trade against an existing offer.
/// The trade tracks the progress through various states until completion or cancellation.
/// 
/// # Trade Lifecycle
/// 1. Initiated: Trade created, waiting for payment confirmations
/// 2. PaymentConfirmed: Both parties confirmed payment, USDC ready for release
/// 3. Completed: USDC released to buyer, trade successful
/// 4. Cancelled: Trade cancelled, USDC returned to seller
/// 5. Disputed: Conflict raised, requires admin intervention
/// 
/// # Security Features
/// - Start time enables expiration checking to prevent indefinite escrow
/// - Separate confirmation flags prevent single-party manipulation
/// - Status tracking ensures proper state transitions
/// - Immutable offer_id links trade to specific offer terms
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trade {
    /// The ID of the offer this trade is executing against
    /// Links this trade to specific offer terms (amounts, seller, etc.)
    pub offer_id: u64,
    
    /// The address of the buyer initiating this trade
    /// This address will receive the USDC upon successful completion
    pub buyer: Address,
    
    /// Timestamp when the trade was initiated (in seconds since epoch)
    /// Used for calculating trade expiration and timeout handling
    /// Prevents trades from staying active indefinitely
    pub start_time: u64,
    
    /// Current status of the trade in its lifecycle
    /// Determines what operations are allowed and what happens next
    pub status: TradeStatus,
    
    /// Whether the buyer has confirmed sending KES payment off-chain
    /// Buyer sets this to true after sending KES via traditional payment methods
    /// Part of the dual-confirmation system for trade completion
    pub buyer_confirmed_payment: bool,
    
    /// Whether the seller has confirmed receiving KES payment off-chain
    /// Seller sets this to true after receiving and verifying KES payment
    /// When both buyer and seller confirm, USDC is automatically released
    pub seller_confirmed_payment: bool,
}

// ================================================================================================
// ENUMERATIONS
// ================================================================================================

/// Represents the current state of a trade in its lifecycle.
/// 
/// The status determines which operations are allowed and guides the trade flow.
/// State transitions are carefully controlled to prevent invalid operations.
/// 
/// # State Transition Rules
/// - Initiated → PaymentConfirmed (when both parties confirm)
/// - Initiated → Cancelled (by participant request or expiration)
/// - Initiated → Disputed (when conflicts arise)
/// - PaymentConfirmed → Completed (automatic USDC release)
/// - Disputed → Completed or Cancelled (by admin resolution)
/// 
/// # Security Considerations
/// - Final states (Completed, Cancelled) prevent further modifications
/// - Disputed state requires admin intervention to resolve
/// - State changes are irreversible to maintain audit trail
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TradeStatus {
    /// Trade has been created and is waiting for payment confirmations
    /// Both buyer and seller can still cancel at this stage
    /// Trade will expire if confirmations don't happen within time limit
    Initiated,
    
    /// Both buyer and seller have confirmed payment completion
    /// USDC is ready to be released to buyer automatically
    /// This is a brief transitional state before Completed
    PaymentConfirmed,
    
    /// Trade has been successfully completed
    /// USDC has been transferred to buyer, fees collected
    /// This is a final state - no further changes allowed
    Completed,
    
    /// Trade has been cancelled by participants or due to expiration
    /// USDC has been returned to seller
    /// This is a final state - no further changes allowed
    Cancelled,
    
    /// A dispute has been raised and requires admin intervention
    /// No automatic operations can occur until admin resolves the dispute
    /// Admin can choose to complete trade or cancel it
    Disputed,
}

/// Represents admin's decision when resolving a disputed trade.
/// 
/// When trades are disputed, only the admin can resolve them by choosing
/// one of two outcomes based on off-chain investigation.
/// 
/// # Resolution Logic
/// - ReleaseToBuyer: Admin determined payment was successful, complete the trade
/// - RefundToSeller: Admin determined payment failed or was fraudulent, cancel trade
/// 
/// # Fee Handling
/// - ReleaseToBuyer: Normal fees are collected as if trade completed normally
/// - RefundToSeller: No fees collected, full amount returned to seller
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeResolution {
    /// Release escrowed USDC to buyer (minus fees)
    /// Used when admin determines the trade should complete successfully
    /// Fees are collected normally as this counts as a successful trade
    ReleaseToBuyer,
    
    /// Refund full USDC amount to seller (no fees)
    /// Used when admin determines the trade should be cancelled
    /// No fees collected as this is treated as a failed/fraudulent trade
    RefundToSeller,
}

// ================================================================================================
// ERROR DEFINITIONS
// ================================================================================================

/// Comprehensive error types for all possible failure scenarios in the marketplace.
/// 
/// Each error is assigned a unique numeric code for easy identification in logs
/// and client applications. Error codes are grouped logically by function area.
/// 
/// # Error Code Ranges
/// - 1-5: Entity not found errors
/// - 6-10: Authorization and access control errors  
/// - 11-15: Business logic and validation errors
/// - 16-20: Technical and system errors
/// 
/// # Design Principles
/// - Descriptive names that clearly indicate the problem
/// - Unique numeric codes for programmatic handling
/// - Comprehensive coverage of all failure scenarios
/// - Grouped by logical categories for maintainability
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    // ========== Entity Not Found Errors (1-5) ==========
    
    /// Requested offer ID does not exist in the marketplace
    /// This can happen if offer was never created, already cancelled, or completed
    OfferNotFound = 1,
    
    /// Requested trade ID does not exist in the marketplace
    /// This can happen if trade was never created or ID is invalid
    TradeNotFound = 2,
    
    // ========== Business Rule Violations (3-7) ==========
    
    /// Seller already has an active offer and cannot create another
    /// Business rule: one active offer per seller to prevent liquidity fragmentation
    AlreadyHasActiveOffer = 3,
    
    /// Trade has exceeded its time limit and is no longer valid
    /// Trades expire to prevent indefinite escrow situations
    TradeExpired = 4,
    
    /// Operation is not allowed for the current trade status
    /// Each trade status has specific allowed operations
    InvalidTradeStatus = 5,
    
    /// Caller is not authorized to perform this operation
    /// Used for access control and participant validation
    Unauthorized = 6,
    
    /// Offer already has an active trade and cannot accept another
    /// Business rule: one trade per offer to maintain order
    TradeAlreadyInitiated = 7,
    
    /// Contract is paused and trading operations are disabled
    /// Emergency mechanism for maintenance or security issues
    ContractPaused = 8,
    
    /// Trade has not yet expired (opposite of TradeExpired)
    /// Used when trying to resolve non-expired trades
    TradeNotExpired = 9,
    
    // ========== Token and Financial Errors (10-14) ==========
    
    /// Seller has insufficient USDC balance or hasn't approved contract spending
    /// Common when seller doesn't have enough tokens or hasn't called approve()
    InsufficientAllowance = 10,
    
    /// Input amount is invalid (negative, zero, or outside allowed ranges)
    /// Used for all amount validation throughout the contract
    InvalidAmount = 11,
    
    /// Token transfer operation failed for technical reasons
    /// Could indicate network issues, token contract problems, or insufficient gas
    TokenTransferFailed = 12,
    
    /// Provided token address is not a valid token contract
    /// Used during initialization to validate USDC token address
    InvalidTokenAddress = 13,
    
    /// User has exceeded rate limits for operations
    /// Anti-spam mechanism to prevent abuse (future enhancement)
    RateLimitExceeded = 14,
}

// ================================================================================================
// EVENT CONSTANTS
// ================================================================================================
// These symbols are used for emitting events that provide transparency and enable
// off-chain indexing and monitoring of marketplace activities.

/// Event emitted when a new offer is created
/// Contains: (offer_id, usdc_amount, kes_amount)
/// Used by: create_offer function
pub const OFFER_CREATED: Symbol = Symbol::short("offr_crt");

/// Event emitted when a trade is initiated against an offer
/// Contains: (trade_id, offer_id)  
/// Used by: initiate_trade function
pub const TRADE_INITIATED: Symbol = Symbol::short("trd_init");

/// Event emitted when a participant confirms payment
/// Contains: (trade_id)
/// Used by: confirm_payment function
pub const PAYMENT_CONFIRMED: Symbol = Symbol::short("pay_conf");

/// Event emitted when a trade is successfully completed
/// Contains: (trade_id)
/// Used by: release_usdc function (internal)
pub const TRADE_COMPLETED: Symbol = Symbol::short("trd_comp");

/// Event emitted when a trade is cancelled
/// Contains: (trade_id)
/// Used by: cancel_trade, resolve_expired_trade functions
pub const TRADE_CANCELLED: Symbol = Symbol::short("trd_canc");

/// Event emitted when an offer is cancelled
/// Contains: (offer_id)
/// Used by: cancel_offer function
pub const OFFER_CANCELLED: Symbol = Symbol::short("offr_canc");

/// Event emitted when a dispute is raised for a trade
/// Contains: (trade_id)
/// Used by: raise_dispute function
pub const DISPUTE_RAISED: Symbol = Symbol::short("dis_rais");

/// Event emitted when an admin resolves a dispute
/// Contains: (trade_id, resolution)
/// Used by: resolve_dispute function
pub const DISPUTE_RESOLVED: Symbol = Symbol::short("dis_resl");
