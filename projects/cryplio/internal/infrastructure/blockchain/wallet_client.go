package blockchain

import (
	"context"
	wallet "cryplio/internal/domain/wallet"
)

// NoopWalletClient is a stub until a chain wallet provider is integrated.
type NoopWalletClient struct{}

func NewNoopWalletClient() *NoopWalletClient {
	return &NoopWalletClient{}
}

func (c *NoopWalletClient) CreateDepositAddress(ctx context.Context, cryptoID int, label string) (string, error) {
	// Return a unique address for each label to avoid unique constraint violations in DB
	return "0xnoop-" + label, nil
}

func (c *NoopWalletClient) Send(context.Context, *wallet.WalletTransaction, string) (string, error) {
	return "0xnoop-tx", nil
}

func (c *NoopWalletClient) Watch(context.Context, string) error {
	return nil
}
