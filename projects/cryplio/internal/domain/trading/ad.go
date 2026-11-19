package trading

import (
	"context"
	"errors"

	"github.com/google/uuid"
)

// ListActiveAds returns all active ads that match the given filter.
func (s *tradeService) ListActiveAds(ctx context.Context, filter AdFilter) ([]TradeAd, int, error) {
	status := TradeAdStatusActive
	isPaused := false
	filter.Status = &status
	filter.IsPaused = &isPaused

	return s.tradeRepo.ListAds(ctx, filter)
}

// GetAd returns a single ad by its ID.
func (s *tradeService) GetAd(ctx context.Context, id uuid.UUID) (*TradeAd, error) {
	return s.tradeRepo.GetAdByID(ctx, id)
}

// CreateAd persists a new ad, generating a UUID when none is provided.
func (s *tradeService) CreateAd(ctx context.Context, ad *TradeAd) error {
	if ad.AdID == uuid.Nil {
		ad.AdID = uuid.New()
	}
	ad.PaymentWindowMinutes = 60
	return s.tradeRepo.CreateAd(ctx, ad)
}

// UpdateAd applies a partial update to an existing ad owned by userID.
func (s *tradeService) UpdateAd(ctx context.Context, adID, userID uuid.UUID, updates *TradeAd) error {
	ad, err := s.tradeRepo.GetAdByID(ctx, adID)
	if err != nil {
		return err
	}
	if ad == nil {
		return errors.New("ad not found")
	}
	if ad.UserID != userID {
		return errors.New("unauthorized")
	}

	if updates.Type != "" {
		ad.Type = updates.Type
	}
	if updates.CryptoID != 0 {
		ad.CryptoID = updates.CryptoID
	}
	if updates.FiatID != 0 {
		ad.FiatID = updates.FiatID
	}
	if updates.Price > 0 {
		ad.Price = updates.Price
	}
	if updates.MinAmount > 0 {
		ad.MinAmount = updates.MinAmount
	}
	if updates.MaxAmount > 0 {
		ad.MaxAmount = updates.MaxAmount
	}
	if len(updates.PaymentMethods) > 0 {
		ad.PaymentMethods = updates.PaymentMethods
	}
	if updates.TradeTerms != nil {
		ad.TradeTerms = updates.TradeTerms
	}
	ad.PaymentWindowMinutes = 60

	return s.tradeRepo.UpdateAd(ctx, ad)
}

// DeleteAd removes an ad owned by userID.
func (s *tradeService) DeleteAd(ctx context.Context, adID, userID uuid.UUID) error {
	ad, err := s.tradeRepo.GetAdByID(ctx, adID)
	if err != nil {
		return err
	}
	if ad == nil {
		return errors.New("ad not found")
	}
	if ad.UserID != userID {
		return errors.New("unauthorized")
	}
	return s.tradeRepo.DeleteAd(ctx, adID)
}

// ListUserAds returns all ads belonging to the given user.
func (s *tradeService) ListUserAds(ctx context.Context, userID uuid.UUID) ([]TradeAd, int, error) {
	filter := AdFilter{
		UserID: &userID,
		Limit:  100, // Show all for now.
	}
	return s.tradeRepo.ListAds(ctx, filter)
}

// ToggleAdStatus flips the paused/active state of an ad owned by userID.
func (s *tradeService) ToggleAdStatus(ctx context.Context, adID, userID uuid.UUID) error {
	ad, err := s.tradeRepo.GetAdByID(ctx, adID)
	if err != nil {
		return err
	}
	if ad == nil {
		return errors.New("ad not found")
	}
	if ad.UserID != userID {
		return errors.New("unauthorized")
	}

	if ad.IsPaused {
		ad.Resume()
	} else {
		ad.Pause()
	}
	return s.tradeRepo.UpdateAd(ctx, ad)
}
