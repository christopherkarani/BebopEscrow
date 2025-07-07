# iOS Swift Integration Guide for P2P Marketplace Smart Contract

This guide provides a comprehensive integration plan for connecting your iOS Swift app to the P2P Marketplace smart contract on the Stellar blockchain using the Stellar SDK.

## Prerequisites

1. **Stellar SDK for iOS**: Install via Swift Package Manager or CocoaPods
2. **Soroban SDK**: For smart contract interactions
3. **USDC Token Contract Address**: The deployed USDC token on Stellar
4. **P2P Marketplace Contract Address**: Your deployed marketplace contract

## Installation

### Swift Package Manager

```swift
dependencies: [
    .package(url: "https://github.com/Soneso/stellar-ios-mac-sdk.git", from: "2.5.0"),
    .package(url: "https://github.com/Soneso/soroban-ios-sdk.git", from: "0.1.0")
]
```

### CocoaPods

```ruby
pod 'stellar-ios-mac-sdk'
pod 'soroban-ios-sdk'
```

## Core Integration Components

### 1. Contract Service Manager

```swift
import StellarSDK
import SorobanSDK

class P2PMarketplaceService {
    private let sdk: StellarSDK
    private let sorobanServer: SorobanServer
    private let contractAddress: String
    private let usdcTokenAddress: String
    private let networkPassphrase: String
    
    init(
        horizonURL: String,
        sorobanURL: String,
        contractAddress: String,
        usdcTokenAddress: String,
        networkPassphrase: String
    ) {
        self.sdk = StellarSDK(withHorizonUrl: horizonURL)
        self.sorobanServer = SorobanServer(endpoint: sorobanURL)
        self.contractAddress = contractAddress
        self.usdcTokenAddress = usdcTokenAddress
        self.networkPassphrase = networkPassphrase
    }
}
```

### 2. Data Models

```swift
// MARK: - Contract Data Models

struct Offer {
    let id: UInt64
    let seller: String
    let usdcAmount: Int128
    let kesAmount: Int128
}

enum TradeStatus: String {
    case initiated = "Initiated"
    case paymentConfirmed = "PaymentConfirmed"
    case completed = "Completed"
    case cancelled = "Cancelled"
    case disputed = "Disputed"
}

struct Trade {
    let id: UInt64
    let offerId: UInt64
    let buyer: String
    let startTime: UInt64
    let status: TradeStatus
    let buyerConfirmedPayment: Bool
    let sellerConfirmedPayment: Bool
}

enum DisputeResolution: String {
    case releaseToBuyer = "ReleaseToBuyer"
    case refundToSeller = "RefundToSeller"
}

// MARK: - Error Handling

enum P2PMarketplaceError: Error {
    case offerNotFound
    case tradeNotFound
    case alreadyHasActiveOffer
    case tradeExpired
    case invalidTradeStatus
    case unauthorized
    case tradeAlreadyInitiated
    case contractPaused
    case tradeNotExpired
    case insufficientAllowance
    case invalidAmount
    case tokenTransferFailed
    case invalidTokenAddress
    case rateLimitExceeded
    case networkError(String)
    case contractError(String)
}
```

### 3. Contract Initialization

```swift
extension P2PMarketplaceService {
    func initialize(
        adminAddress: String,
        feeCollectorAddress: String,
        signerKeyPair: KeyPair
    ) async throws -> TransactionResult {
        let contract = try Address(contractId: contractAddress)
        let admin = try Address(accountId: adminAddress)
        let usdcToken = try Address(contractId: usdcTokenAddress)
        let feeCollector = try Address(accountId: feeCollectorAddress)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "initialize",
                functionArguments: [
                    .address(admin),
                    .address(usdcToken),
                    .address(feeCollector)
                ]
            )
        
        return try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: signerKeyPair
        )
    }
}
```

### 4. USDC Token Approval

Before creating offers, users must approve the marketplace contract to spend their USDC:

```swift
extension P2PMarketplaceService {
    func approveUSDCSpending(
        amount: Int128,
        signerKeyPair: KeyPair
    ) async throws -> TransactionResult {
        let usdcContract = try Address(contractId: usdcTokenAddress)
        let marketplace = try Address(contractId: contractAddress)
        let spender = try Address(accountId: signerKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: usdcContract,
                functionName: "approve",
                functionArguments: [
                    .address(spender),
                    .address(marketplace),
                    .i128(amount),
                    .u32(3153600000) // Expiration ledger (~ 1 year)
                ]
            )
        
        return try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: signerKeyPair
        )
    }
}
```

### 5. Core Marketplace Functions

```swift
extension P2PMarketplaceService {
    // MARK: - Create Offer
    func createOffer(
        usdcAmount: Int128,
        kesAmount: Int128,
        sellerKeyPair: KeyPair
    ) async throws -> UInt64 {
        let contract = try Address(contractId: contractAddress)
        let seller = try Address(accountId: sellerKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "create_offer",
                functionArguments: [
                    .address(seller),
                    .i128(usdcAmount),
                    .i128(kesAmount)
                ]
            )
        
        let result = try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: sellerKeyPair
        )
        
        // Extract offer ID from result
        return try extractOfferId(from: result)
    }
    
    // MARK: - Initiate Trade
    func initiateTrade(
        offerId: UInt64,
        buyerKeyPair: KeyPair
    ) async throws -> UInt64 {
        let contract = try Address(contractId: contractAddress)
        let buyer = try Address(accountId: buyerKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "initiate_trade",
                functionArguments: [
                    .address(buyer),
                    .u64(offerId)
                ]
            )
        
        let result = try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: buyerKeyPair
        )
        
        return try extractTradeId(from: result)
    }
    
    // MARK: - Confirm Payment
    func confirmPayment(
        tradeId: UInt64,
        participantKeyPair: KeyPair
    ) async throws -> TransactionResult {
        let contract = try Address(contractId: contractAddress)
        let participant = try Address(accountId: participantKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "confirm_payment",
                functionArguments: [
                    .u64(tradeId),
                    .address(participant)
                ]
            )
        
        return try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: participantKeyPair
        )
    }
    
    // MARK: - Cancel Offer
    func cancelOffer(
        offerId: UInt64,
        sellerKeyPair: KeyPair
    ) async throws -> TransactionResult {
        let contract = try Address(contractId: contractAddress)
        let seller = try Address(accountId: sellerKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "cancel_offer",
                functionArguments: [
                    .address(seller),
                    .u64(offerId)
                ]
            )
        
        return try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: sellerKeyPair
        )
    }
    
    // MARK: - Cancel Trade
    func cancelTrade(
        tradeId: UInt64,
        participantKeyPair: KeyPair
    ) async throws -> TransactionResult {
        let contract = try Address(contractId: contractAddress)
        let participant = try Address(accountId: participantKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "cancel_trade",
                functionArguments: [
                    .u64(tradeId),
                    .address(participant)
                ]
            )
        
        return try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: participantKeyPair
        )
    }
    
    // MARK: - Raise Dispute
    func raiseDispute(
        tradeId: UInt64,
        callerKeyPair: KeyPair
    ) async throws -> TransactionResult {
        let contract = try Address(contractId: contractAddress)
        let caller = try Address(accountId: callerKeyPair.accountId)
        
        let invokeOperation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "raise_dispute",
                functionArguments: [
                    .u64(tradeId),
                    .address(caller)
                ]
            )
        
        return try await submitTransaction(
            operation: invokeOperation,
            signerKeyPair: callerKeyPair
        )
    }
}
```

### 6. Query Functions

```swift
extension P2PMarketplaceService {
    // MARK: - Get Offer
    func getOffer(offerId: UInt64) async throws -> Offer? {
        let contract = try Address(contractId: contractAddress)
        
        let operation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "get_offer",
                functionArguments: [.u64(offerId)]
            )
        
        let response = try await sorobanServer.simulateTransaction(
            operation: operation,
            sourceAccount: getViewAccount()
        )
        
        return try parseOffer(from: response)
    }
    
    // MARK: - Get Trade
    func getTrade(tradeId: UInt64) async throws -> Trade? {
        let contract = try Address(contractId: contractAddress)
        
        let operation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "get_trade",
                functionArguments: [.u64(tradeId)]
            )
        
        let response = try await sorobanServer.simulateTransaction(
            operation: operation,
            sourceAccount: getViewAccount()
        )
        
        return try parseTrade(from: response)
    }
    
    // MARK: - Get Active Offer for Seller
    func getSellerActiveOffer(sellerAddress: String) async throws -> UInt64? {
        let contract = try Address(contractId: contractAddress)
        let seller = try Address(accountId: sellerAddress)
        
        let operation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "get_seller_active_offer",
                functionArguments: [.address(seller)]
            )
        
        let response = try await sorobanServer.simulateTransaction(
            operation: operation,
            sourceAccount: getViewAccount()
        )
        
        return try parseOfferId(from: response)
    }
    
    // MARK: - Get Contract Info
    func getContractInfo() async throws -> ContractInfo {
        let contract = try Address(contractId: contractAddress)
        
        let operation = try InvokeHostFunctionOperation
            .forInvokeContractFunction(
                contractAddress: contract,
                functionName: "get_contract_info",
                functionArguments: []
            )
        
        let response = try await sorobanServer.simulateTransaction(
            operation: operation,
            sourceAccount: getViewAccount()
        )
        
        return try parseContractInfo(from: response)
    }
}
```

### 7. Transaction Management

```swift
extension P2PMarketplaceService {
    private func submitTransaction(
        operation: Operation,
        signerKeyPair: KeyPair
    ) async throws -> TransactionResult {
        // Get account details
        let sourceAccount = try await sdk.accounts
            .getAccountDetails(accountId: signerKeyPair.accountId)
        
        // Build transaction
        let transaction = try TransactionBuilder(
            sourceAccount: sourceAccount,
            network: Network(passphrase: networkPassphrase)
        )
        .add(operation: operation)
        .build()
        
        // Sign transaction
        try transaction.sign(keyPair: signerKeyPair, network: Network(passphrase: networkPassphrase))
        
        // Submit to Soroban
        let preparedTx = try await sorobanServer.prepareTransaction(transaction)
        
        // Submit to network
        return try await sdk.transactions.submitTransaction(transaction: preparedTx)
    }
    
    private func getViewAccount() -> Account {
        // Create a dummy account for view-only calls
        return Account(
            keyPair: try! KeyPair.random(),
            sequenceNumber: 0
        )
    }
}
```

### 8. Event Monitoring

```swift
extension P2PMarketplaceService {
    enum MarketplaceEvent {
        case offerCreated(sellerId: String, offerId: UInt64, usdcAmount: Int128, kesAmount: Int128)
        case tradeInitiated(buyerId: String, tradeId: UInt64, offerId: UInt64)
        case paymentConfirmed(participantId: String, tradeId: UInt64)
        case tradeCompleted(buyerId: String, tradeId: UInt64)
        case tradeCancelled(participantId: String, tradeId: UInt64)
        case offerCancelled(sellerId: String, offerId: UInt64)
        case disputeRaised(callerId: String, tradeId: UInt64)
        case disputeResolved(tradeId: UInt64, resolution: DisputeResolution)
    }
    
    func subscribeToEvents(
        onEvent: @escaping (MarketplaceEvent) -> Void
    ) async throws {
        // Subscribe to contract events via Soroban events API
        let eventSubscription = try await sorobanServer.subscribeToContractEvents(
            contractId: contractAddress,
            startLedger: nil
        ) { event in
            if let marketplaceEvent = self.parseEvent(event) {
                onEvent(marketplaceEvent)
            }
        }
        
        // Store subscription for cleanup
        self.eventSubscription = eventSubscription
    }
    
    private func parseEvent(_ event: ContractEvent) -> MarketplaceEvent? {
        // Parse Soroban contract events into app events
        // Implementation depends on event structure
        return nil
    }
}
```

### 9. UI Integration Example

```swift
import SwiftUI

class P2PMarketplaceViewModel: ObservableObject {
    private let marketplaceService: P2PMarketplaceService
    
    @Published var offers: [Offer] = []
    @Published var activeTrades: [Trade] = []
    @Published var isLoading = false
    @Published var error: P2PMarketplaceError?
    
    init(marketplaceService: P2PMarketplaceService) {
        self.marketplaceService = marketplaceService
    }
    
    // MARK: - Create Offer Flow
    func createOffer(usdcAmount: Decimal, kesAmount: Decimal) async {
        await MainActor.run { 
            self.isLoading = true 
            self.error = nil
        }
        
        do {
            // Convert decimals to contract amounts (considering 6 decimals for USDC)
            let usdcAmountScaled = Int128(truncating: (usdcAmount * 1_000_000) as NSNumber)
            let kesAmountScaled = Int128(truncating: (kesAmount * 100) as NSNumber) // Assuming 2 decimals for KES
            
            // First approve USDC spending
            let keyPair = getUserKeyPair() // Get from secure storage
            _ = try await marketplaceService.approveUSDCSpending(
                amount: usdcAmountScaled,
                signerKeyPair: keyPair
            )
            
            // Then create offer
            let offerId = try await marketplaceService.createOffer(
                usdcAmount: usdcAmountScaled,
                kesAmount: kesAmountScaled,
                sellerKeyPair: keyPair
            )
            
            await MainActor.run {
                // Update UI with new offer
                self.isLoading = false
            }
            
        } catch {
            await MainActor.run {
                self.error = error as? P2PMarketplaceError ?? .networkError(error.localizedDescription)
                self.isLoading = false
            }
        }
    }
}

// MARK: - SwiftUI View Example
struct CreateOfferView: View {
    @StateObject private var viewModel: P2PMarketplaceViewModel
    @State private var usdcAmount = ""
    @State private var kesAmount = ""
    @State private var exchangeRate: Decimal = 150.0 // KES per USDC
    
    var body: some View {
        Form {
            Section("Offer Details") {
                TextField("USDC Amount", text: $usdcAmount)
                    .keyboardType(.decimalPad)
                    .onChange(of: usdcAmount) { newValue in
                        updateKesAmount()
                    }
                
                TextField("KES Amount", text: $kesAmount)
                    .keyboardType(.decimalPad)
                
                Text("Exchange Rate: 1 USDC = \(exchangeRate) KES")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
            
            Section {
                Button("Create Offer") {
                    Task {
                        await viewModel.createOffer(
                            usdcAmount: Decimal(string: usdcAmount) ?? 0,
                            kesAmount: Decimal(string: kesAmount) ?? 0
                        )
                    }
                }
                .disabled(usdcAmount.isEmpty || kesAmount.isEmpty || viewModel.isLoading)
            }
        }
        .navigationTitle("Create Offer")
        .alert("Error", isPresented: .constant(viewModel.error != nil)) {
            Button("OK") {
                viewModel.error = nil
            }
        } message: {
            Text(viewModel.error?.localizedDescription ?? "Unknown error")
        }
    }
    
    private func updateKesAmount() {
        if let usdc = Decimal(string: usdcAmount) {
            let kes = usdc * exchangeRate
            kesAmount = "\(kes)"
        }
    }
}
```

## Security Best Practices

1. **Key Management**
   - Use iOS Keychain for storing private keys
   - Implement biometric authentication for transaction signing
   - Never expose private keys in logs or UI

2. **Transaction Validation**
   - Always verify transaction details before signing
   - Implement transaction limits and confirmations
   - Show clear UI for what the user is approving

3. **Error Handling**
   - Implement comprehensive error handling
   - Provide clear user feedback
   - Log errors securely without exposing sensitive data

4. **Network Security**
   - Use HTTPS for all API calls
   - Implement certificate pinning
   - Handle network failures gracefully

## Testing Recommendations

1. **Unit Tests**
   - Test all contract interaction methods
   - Mock Soroban responses
   - Test error scenarios

2. **Integration Tests**
   - Test on Stellar testnet first
   - Test all user flows end-to-end
   - Test edge cases and error conditions

3. **Performance Testing**
   - Test with various network conditions
   - Monitor memory usage
   - Optimize for battery efficiency

## Deployment Checklist

- [ ] Deploy and verify smart contract on mainnet
- [ ] Update contract addresses in app configuration
- [ ] Implement proper error tracking (e.g., Sentry)
- [ ] Set up monitoring for contract events
- [ ] Implement rate limiting on client side
- [ ] Add analytics for user behavior
- [ ] Prepare customer support documentation
- [ ] Test disaster recovery procedures

## Conclusion

This integration guide provides a comprehensive foundation for connecting your iOS app to the P2P Marketplace smart contract. The modular architecture allows for easy extension and maintenance while following Swift best practices and ensuring security throughout the implementation.