import { useState } from 'react';
import { Card } from '@shared/ui/Card';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { toast } from '@shared/ui/Toast';
import { pauseBot, stopBot } from '../../api';
import type { Bot } from '../../types';
import styles from './BotCard.module.css';

interface BotCardProps {
  bot: Bot;
  onUpdated: () => void;
}

function statusVariant(status: Bot['status']): BadgeVariant {
  if (status === 'active') return 'success';
  if (status === 'paused') return 'warning';
  return 'neutral';
}

function typeLabel(type: Bot['type']): string {
  const labels: Record<Bot['type'], string> = {
    grid: 'Grid',
    dca: 'DCA',
    infinity_grid: 'Infinity Grid',
    futures_grid: 'Futures Grid',
    arb: 'Arbitrage',
    martingale: 'Martingale',
  };
  return labels[type];
}

function pnlVariant(pnl: string): string {
  const num = parseFloat(pnl);
  if (num > 0) return styles.pnlPositive;
  if (num < 0) return styles.pnlNegative;
  return styles.pnlNeutral;
}

export function BotCard({ bot, onUpdated }: BotCardProps) {
  const [pausing, setPausing] = useState(false);
  const [stopTarget, setStopTarget] = useState(false);
  const [stopping, setStopping] = useState(false);

  async function handlePause() {
    setPausing(true);
    try {
      await pauseBot(bot.bot_id);
      toast.success(`Bot ${bot.status === 'paused' ? 'resumed' : 'paused'}`);
      onUpdated();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Action failed');
    } finally {
      setPausing(false);
    }
  }

  async function handleStop() {
    setStopping(true);
    try {
      await stopBot(bot.bot_id);
      toast.success('Bot stopped');
      onUpdated();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to stop bot');
    } finally {
      setStopping(false);
      setStopTarget(false);
    }
  }

  const isActive = bot.status !== 'stopped';

  return (
    <>
      <Card className={styles.card}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <Badge variant="info" size="sm">{typeLabel(bot.type)}</Badge>
            <span className={styles.symbol}>{bot.symbol}</span>
          </div>
          <Badge variant={statusVariant(bot.status)} size="sm">
            {bot.status}
          </Badge>
        </div>

        <div className={styles.pnlRow}>
          <span className={styles.pnlLabel}>PnL</span>
          <span className={pnlVariant(bot.realized_pnl)}>
            {parseFloat(bot.realized_pnl) >= 0 ? '+' : ''}{bot.realized_pnl} USDT
          </span>
        </div>

        <div className={styles.footer}>
          <span className={styles.date}>
            {new Date(bot.created_at).toLocaleDateString()}
          </span>
          {isActive && (
            <div className={styles.actions}>
              <Button
                variant="ghost"
                size="sm"
                loading={pausing}
                onClick={() => { void handlePause(); }}
              >
                {bot.status === 'paused' ? 'Resume' : 'Pause'}
              </Button>
              <Button
                variant="danger"
                size="sm"
                onClick={() => setStopTarget(true)}
              >
                Stop
              </Button>
            </div>
          )}
        </div>
      </Card>

      <ConfirmDialog
        open={stopTarget && !stopping}
        onConfirm={() => { void handleStop(); }}
        onCancel={() => setStopTarget(false)}
        title="Stop Bot"
        message={`Stop this ${typeLabel(bot.type)} bot on ${bot.symbol}? This cannot be undone.`}
        confirmText="Stop Bot"
        variant="danger"
      />
    </>
  );
}
