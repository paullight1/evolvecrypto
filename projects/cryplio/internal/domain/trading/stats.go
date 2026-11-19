package trading

import (
	"context"
	"cryplio/internal/domain/identity"
	"time"

	"github.com/google/uuid"
)

// recalculateUserStats updates a user's aggregated statistics based on their trade history.
// It's called after major trade events (completion, feedback, dispute).
func (s *tradeService) recalculateUserStats(ctx context.Context, userID uuid.UUID) error {
	// 1. Fetch current stats (fallback to mapping from 0 if missing)
	stats, err := s.identityRepo.GetStats(ctx, userID)
	if err != nil {
		return err
	}
	if stats == nil {
		stats = &identity.UserStats{UserID: userID}
	}

	// 2. Fetch all trades for this user.
	// Since we need to calculate averages, we need completed trades.
	trades, err := s.tradeRepo.ListTrades(ctx, userID, "all")
	if err != nil {
		return err
	}

	totalTrades := len(trades)
	successfulTrades := 0
	disputedTrades := 0
	totalVolume := 0.0
	var lastTradeAt *time.Time

	// Speed metrics sums
	var totalPaymentTimeSeconds int64
	var paidTradesCount int64
	var totalReleaseTimeSeconds int64
	var releasedTradesCount int64

	for _, t := range trades {
		totalVolume += t.FiatAmount
		if t.Status == TradeStatusCompleted || t.Status == TradeStatusReleased {
			successfulTrades++
		}
		if t.Status == TradeStatusDisputed {
			disputedTrades++
		}
		if lastTradeAt == nil || t.CreatedAt.After(*lastTradeAt) {
			lastTradeAt = &t.CreatedAt
		}

		// Payment time (MarkedAsPaid - CreatedAt)
		if t.PaymentMarkedAt != nil && t.StartedAt != nil {
			diff := t.PaymentMarkedAt.Sub(*t.StartedAt).Seconds()
			if diff > 0 {
				totalPaymentTimeSeconds += int64(diff)
				paidTradesCount++
			}
		}

		// Release time (ReleasedAt - MarkedAsPaid)
		if t.ReleasedAt != nil && t.PaymentMarkedAt != nil {
			diff := t.ReleasedAt.Sub(*t.PaymentMarkedAt).Seconds()
			if diff > 0 {
				totalReleaseTimeSeconds += int64(diff)
				releasedTradesCount++
			}
		}
	}

	stats.TotalTrades = totalTrades
	stats.SuccessfulTrades = successfulTrades
	stats.TotalVolumeUSD = totalVolume
	stats.LastTradeAt = lastTradeAt

	if totalTrades > 0 {
		stats.DisputeRate = (float64(disputedTrades) / float64(totalTrades)) * 100.0
	}

	if paidTradesCount > 0 {
		stats.AvgPaymentTime = int(totalPaymentTimeSeconds / paidTradesCount)
	}
	if releasedTradesCount > 0 {
		stats.AvgReleaseTime = int(totalReleaseTimeSeconds / releasedTradesCount)
	}

	// 3. Fetch Feedbacks for Rating.
	feedbacks, err := s.tradeRepo.ListUserFeedback(ctx, userID)
	if err == nil && len(feedbacks) > 0 {
		var sumRating float64
		pos, neu, neg := 0, 0, 0
		for _, f := range feedbacks {
			switch f.Rating {
			case FeedbackPositive:
				pos++
				sumRating += 5.0
			case FeedbackNeutral:
				neu++
				sumRating += 3.0
			case FeedbackNegative:
				neg++
				sumRating += 1.0
			}
		}
		avg := sumRating / float64(len(feedbacks))
		stats.AvgRating = &avg
		stats.PositiveFeedbackCount = pos
		stats.NeutralFeedbackCount = neu
		stats.NegativeFeedbackCount = neg
	}

	return s.identityRepo.UpdateStats(ctx, stats)
}
