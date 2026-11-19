package trading

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"cryplio/internal/domain/dispute"
	"cryplio/internal/domain/notification"

	"github.com/google/uuid"
)

// InitiateTrade validates preconditions, locks escrow on-chain, and persists a new Trade.
func (s *tradeService) InitiateTrade(ctx context.Context, adID, buyerID uuid.UUID, amount float64, paymentMethodID int) (*Trade, error) {
	buyer, err := s.identityRepo.GetByID(ctx, buyerID)
	if err != nil {
		return nil, fmt.Errorf("get buyer: %w", err)
	}
	if buyer == nil {
		return nil, errors.New("buyer not found")
	}
	if !buyer.EmailVerified {
		return nil, errors.New("email verification is required before initiating trades")
	}

	// 1. Fetch Ad.
	ad, err := s.tradeRepo.GetAdByID(ctx, adID)
	if err != nil {
		return nil, fmt.Errorf("get ad: %w", err)
	}
	if ad == nil {
		return nil, errors.New("advertisement not found")
	}

	// 2. Validate Ad Status.
	if !ad.IsActive() {
		return nil, errors.New("advertisement is not active")
	}

	// 3. Prevent Self-Trading.
	if ad.UserID == buyerID {
		return nil, errors.New("you cannot trade with your own advertisement")
	}

	// 4. Validate Amount.
	if amount < ad.MinAmount || amount > ad.MaxAmount {
		return nil, fmt.Errorf("amount must be between %.2f and %.2f", ad.MinAmount, ad.MaxAmount)
	}

	now := time.Now().UTC()
	var tradeBuyerID, tradeSellerID uuid.UUID
	if ad.Type == AdTypeBuy {
		tradeBuyerID = ad.UserID
		tradeSellerID = buyerID // The person who clicked "Trade Now" is selling to the ad poster
	} else {
		tradeBuyerID = buyerID // The person who clicked "Trade Now" is buying from the ad poster
		tradeSellerID = ad.UserID
	}

	trade := &Trade{
		TradeID:              uuid.New(),
		AdID:                 ad.AdID,
		BuyerID:              tradeBuyerID,
		SellerID:             tradeSellerID,
		CryptoID:             ad.CryptoID,
		FiatID:               ad.FiatID,
		CryptoAmount:         amount / ad.Price, // Simple calculation for now.
		FiatAmount:           amount,
		ExchangeRate:         ad.Price,
		AgreedPrice:          ad.Price,
		Status:               TradeStatusPending,
		PaymentMethod:        paymentMethodID,
		PaymentWindowMinutes: 60,
		StartedAt:            &now,
	}

	// 5. Fetch and snapshot payment details
	pm, _ := s.platformRepo.GetPaymentMethod(ctx, paymentMethodID)
	if pm != nil {
		trade.PaymentMethodName = pm.Name
		// Fetch seller's specific payment method details
		sellerMethods, _ := s.identityRepo.GetUserPaymentMethods(ctx, tradeSellerID)
		for _, sm := range sellerMethods {
			// Find match by global method code
			if sm.PaymentMethodCode == pm.Code {
				bytes, _ := json.Marshal(sm)
				trade.PaymentDetails = bytes
				break
			}
		}
	}

	// If no specific details found, use a generic description
	if trade.PaymentDetails == nil && pm != nil {
		bytes, _ := json.Marshal(map[string]string{
			"message": fmt.Sprintf("Please ask the seller for their %s details via chat.", pm.Name),
		})
		trade.PaymentDetails = bytes
	}

	// 6. Lock Escrow on Blockchain (Mock or real client)
	txHash, _, err := s.escrowClient.Lock(ctx, trade)
	if err != nil {
		return nil, fmt.Errorf("blockchain escrow lock failed: %w", err)
	}
	// TxHash not stored in current schema; escrow tracked externally
	_ = txHash
	trade.Status = TradeStatusActive // Active once locked.

	if err = s.tradeRepo.CreateTrade(ctx, trade); err != nil {
		return nil, fmt.Errorf("save trade: %w", err)
	}

	// Notify both parties.
	if s.notificationService != nil {
		// Notify Seller
		msgSeller := "A buyer has initiated a new trade for your advertisement."
		sellerData := map[string]interface{}{
			"trade_id": trade.TradeID.String(),
			"role":     "seller",
		}
		_ = s.notificationService.NotifyWithWebSocket(ctx, tradeSellerID, notification.NotificationTypeTradeStarted, "New Trade Started", msgSeller, sellerData)

		// Notify Buyer
		msgBuyer := "Your trade has been started successfully. Please proceed with payment."
		buyerData := map[string]interface{}{
			"trade_id": trade.TradeID.String(),
			"role":     "buyer",
		}
		_ = s.notificationService.NotifyWithWebSocket(ctx, tradeBuyerID, notification.NotificationTypeTradeStarted, "Trade Started", msgBuyer, buyerData)
	}

	return trade, nil
}

// ListTrades returns all trades where userID participated in the given role.
func (s *tradeService) ListTrades(ctx context.Context, userID uuid.UUID, role string) ([]Trade, error) {
	return s.tradeRepo.ListTrades(ctx, userID, role)
}

// ListAllTrades returns every trade matching the optional status filter.
func (s *tradeService) ListAllTrades(ctx context.Context, status string) ([]Trade, error) {
	return s.tradeRepo.ListAllTrades(ctx, status)
}

// CountTrades returns the number of trades that match the optional status filter.
func (s *tradeService) CountTrades(ctx context.Context, status string) (int, error) {
	return s.tradeRepo.CountTrades(ctx, status)
}

// GetTrade returns a single trade by its ID, enriched with usernames and symbols.
func (s *tradeService) GetTrade(ctx context.Context, id uuid.UUID) (*Trade, error) {
	trade, err := s.tradeRepo.GetTradeByID(ctx, id)
	if err != nil || trade == nil {
		return trade, err
	}

	// Fetch Seller username
	seller, _ := s.identityRepo.GetByID(ctx, trade.SellerID)
	if seller != nil {
		trade.SellerUsername = seller.Username
	}

	// Fetch Crypto and Fiat symbols directly
	cryptoAsync, _ := s.platformRepo.GetCryptoAsset(ctx, trade.CryptoID)
	if cryptoAsync != nil {
		trade.CryptoSymbol = cryptoAsync.Symbol
	}
	fiatAsync, _ := s.platformRepo.GetFiatCurrency(ctx, trade.FiatID)
	if fiatAsync != nil {
		trade.FiatSymbol = fiatAsync.Code
	}

	// 2. Fetch Buyer username
	buyer, _ := s.identityRepo.GetByID(ctx, trade.BuyerID)
	if buyer != nil {
		trade.BuyerUsername = buyer.Username
	}

	// 3. Calculate timer if active
	if trade.Status == TradeStatusActive && trade.StartedAt != nil {
		expiry := trade.StartedAt.UTC().Add(time.Minute * time.Duration(trade.PaymentWindowMinutes))
		trade.TimerExpiresAt = &expiry
	}

	// 4. Enrich Payment Method
	if trade.PaymentMethod > 0 {
		pm, _ := s.platformRepo.GetPaymentMethod(ctx, trade.PaymentMethod)
		if pm != nil {
			trade.PaymentMethodName = pm.Name
		}
	}

	return trade, nil
}

// MarkAsPaid records that the buyer has sent payment and notifies the seller.
func (s *tradeService) MarkAsPaid(ctx context.Context, tradeID, userID uuid.UUID) error {
	trade, err := s.tradeRepo.GetTradeByID(ctx, tradeID)
	if err != nil {
		return err
	}
	if trade == nil {
		return errors.New("trade not found")
	}
	if trade.BuyerID != userID {
		return errors.New("unauthorized")
	}
	if trade.Status != TradeStatusPending && trade.Status != TradeStatusActive {
		return errors.New("invalid trade status")
	}

	trade.MarkAsPaid()
	if err := s.tradeRepo.UpdateTrade(ctx, trade); err != nil {
		return err
	}

	// Notify seller that buyer marked as paid.
	if s.notificationService != nil {
		msg := "The buyer has marked this trade as paid. Please verify the payment and release escrow."
		data := map[string]interface{}{
			"trade_id": trade.TradeID.String(),
			"status":   "paid",
		}
		_ = s.notificationService.NotifyWithWebSocket(ctx, trade.SellerID, notification.NotificationTypeTradePaid, "Trade Marked as Paid", msg, data)
	}
	return nil
}

// ReleaseEscrow releases on-chain escrow, completes the trade, and notifies both parties.
func (s *tradeService) ReleaseEscrow(ctx context.Context, tradeID, userID uuid.UUID) error {
	trade, err := s.tradeRepo.GetTradeByID(ctx, tradeID)
	if err != nil {
		return err
	}
	if trade == nil {
		return errors.New("trade not found")
	}
	if trade.SellerID != userID {
		return errors.New("unauthorized")
	}
	if trade.Status != TradeStatusPaid {
		return errors.New("trade must be marked as paid before releasing escrow")
	}

	trade.Release()

	// Release Escrow on Blockchain.
	txHash, err := s.escrowClient.Release(ctx, trade)
	if err != nil {
		return fmt.Errorf("blockchain escrow release failed: %w", err)
	}
	// TxHash not stored in current schema; escrow tracked externally
	_ = txHash

	// Auto-complete trade after escrow release.
	trade.Complete()

	if err := s.tradeRepo.UpdateTrade(ctx, trade); err != nil {
		// CRITICAL: Blockchain release succeeded but DB update failed.
		// The trade is now inconsistent. Manual reconciliation required.
		// TODO: Implement outbox/saga pattern to ensure atomicity.
		return fmt.Errorf("CRITICAL: escrow released on-chain (txHash=%s) but DB update failed — manual reconciliation required: %w", txHash, err)
	}

	// Notify both parties that trade is completed.
	if s.notificationService != nil {
		msg := "Trade completed successfully. The escrow has been released."
		data := map[string]interface{}{
			"trade_id": trade.TradeID.String(),
			"status":   "completed",
		}
		_ = s.notificationService.NotifyWithWebSocket(ctx, trade.BuyerID, notification.NotificationTypeTradeCompleted, "Trade Completed", msg, data)
		_ = s.notificationService.NotifyWithWebSocket(ctx, trade.SellerID, notification.NotificationTypeTradeCompleted, "Trade Completed", msg, data)
	}

	// Recalculate stats for both parties
	_ = s.recalculateUserStats(ctx, trade.BuyerID)
	_ = s.recalculateUserStats(ctx, trade.SellerID)

	return nil
}

// CancelTrade cancels a trade and notifies the other party.
func (s *tradeService) CancelTrade(ctx context.Context, tradeID, userID uuid.UUID, reason string) error {
	trade, err := s.tradeRepo.GetTradeByID(ctx, tradeID)
	if err != nil {
		return err
	}
	if trade == nil {
		return errors.New("trade not found")
	}
	if trade.BuyerID != userID && trade.SellerID != userID {
		return errors.New("unauthorized")
	}
	if !trade.CanCancel() {
		return errors.New("trade cannot be cancelled at this stage")
	}

	trade.Cancel(reason)
	if err := s.tradeRepo.UpdateTrade(ctx, trade); err != nil {
		return err
	}

	// Notify the other party.
	if s.notificationService != nil {
		msg := fmt.Sprintf("A trade has been cancelled. Reason: %s", reason)
		otherID := trade.SellerID
		if trade.SellerID == userID {
			otherID = trade.BuyerID
		}
		data := map[string]interface{}{
			"trade_id": trade.TradeID.String(),
			"status":   "cancelled",
			"reason":   reason,
		}
		_ = s.notificationService.NotifyWithWebSocket(ctx, otherID, notification.NotificationTypeTradeCancelled, "Trade Cancelled", msg, data)
	}
	return nil
}

// DisputeTrade opens a dispute record, marks the trade as disputed, and notifies both parties.
func (s *tradeService) DisputeTrade(ctx context.Context, tradeID, userID uuid.UUID, reasonCode string, reasonText string) (*Trade, error) {
	trade, err := s.tradeRepo.GetTradeByID(ctx, tradeID)
	if err != nil {
		return nil, err
	}
	if trade == nil {
		return nil, errors.New("trade not found")
	}
	if trade.BuyerID != userID && trade.SellerID != userID {
		return nil, errors.New("unauthorized")
	}
	if trade.Status != TradeStatusPaid && trade.Status != TradeStatusActive {
		return nil, errors.New("trade cannot be disputed in its current status")
	}

	// Create Dispute record.
	d := &dispute.Dispute{
		DisputeID:  uuid.New(),
		TradeID:    tradeID,
		RaisedBy:   userID,
		ReasonCode: reasonCode,
		ReasonText: &reasonText,
		Status:     dispute.DisputeStatusPending,
	}

	if err := s.disputeRepo.Create(ctx, d); err != nil {
		return nil, fmt.Errorf("create dispute: %w", err)
	}

	// Update Trade status.
	trade.Status = TradeStatusDisputed
	if err := s.tradeRepo.UpdateTrade(ctx, trade); err != nil {
		return nil, fmt.Errorf("update trade status: %w", err)
	}

	// Notify both parties.
	if s.notificationService != nil {
		msg := fmt.Sprintf("A dispute has been raised on this trade. Reason: %s", reasonText)
		otherID := trade.SellerID
		if trade.SellerID == userID {
			otherID = trade.BuyerID
		}
		data := map[string]interface{}{
			"trade_id": trade.TradeID.String(),
			"status":   "disputed",
		}
		_ = s.notificationService.NotifyWithWebSocket(ctx, userID, notification.NotificationTypeTradeDisputed, "Trade Disputed", msg, data)
		_ = s.notificationService.NotifyWithWebSocket(ctx, otherID, notification.NotificationTypeTradeDisputed, "Trade Disputed", msg, data)
	}

	return trade, nil
}

// ReconcileExpiredTrades marks pending/active trades whose payment window has passed as expired.
func (s *tradeService) ReconcileExpiredTrades(ctx context.Context) (int, error) {
	now := time.Now().UTC()
	trades, err := s.tradeRepo.ListExpiredPendingTrades(ctx, now)
	if err != nil {
		return 0, err
	}

	updated := 0
	for i := range trades {
		trade := trades[i]
		if trade.Status != TradeStatusPending && trade.Status != TradeStatusActive {
			continue
		}

		trade.Status = TradeStatusExpired

		if err := s.tradeRepo.UpdateTrade(ctx, &trade); err != nil {
			return updated, err
		}
		updated++
	}

	return updated, nil
}
