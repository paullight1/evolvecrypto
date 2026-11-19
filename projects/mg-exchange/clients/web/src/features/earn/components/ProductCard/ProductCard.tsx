import { Card } from '@shared/ui/Card';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import type { EarnProduct } from '../../types';
import styles from './ProductCard.module.css';

interface ProductCardProps {
  product: EarnProduct;
  onSubscribe: (product: EarnProduct) => void;
}

function typeVariant(type: EarnProduct['productType']): BadgeVariant {
  if (type === 'flexible') return 'info';
  if (type === 'fixed_30d') return 'warning';
  return 'success';
}

function typeLabel(type: EarnProduct['productType']): string {
  if (type === 'flexible') return 'Flexible';
  if (type === 'fixed_30d') return 'Fixed 30d';
  return 'Staking';
}

export function ProductCard({ product, onSubscribe }: ProductCardProps) {
  return (
    <Card className={styles.card}>
      <div className={styles.header}>
        <span className={styles.asset}>{product.coin}</span>
        <Badge variant={typeVariant(product.productType)} size="sm">
          {typeLabel(product.productType)}
        </Badge>
      </div>

      <div className={styles.apy}>
        <span className={styles.apyValue}>{product.apy}</span>
        <span className={styles.apyLabel}>APY</span>
      </div>

      <div className={styles.details}>
        <div className={styles.detailRow}>
          <span className={styles.detailLabel}>Min. Amount</span>
          <span className={styles.detailValue}>
            {product.minAmount} {product.coin}
          </span>
        </div>
        {product.lockDays > 0 && (
          <div className={styles.detailRow}>
            <span className={styles.detailLabel}>Lock Period</span>
            <span className={styles.detailValue}>{product.lockDays} days</span>
          </div>
        )}
        {product.lockDays === 0 && (
          <div className={styles.detailRow}>
            <span className={styles.detailLabel}>Lock Period</span>
            <span className={styles.detailValue}>None</span>
          </div>
        )}
      </div>

      <Button variant="primary" fullWidth onClick={() => onSubscribe(product)}>
        Subscribe
      </Button>
    </Card>
  );
}
