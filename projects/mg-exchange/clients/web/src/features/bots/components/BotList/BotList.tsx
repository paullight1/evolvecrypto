import { BotCard } from '../BotCard';
import type { Bot } from '../../types';
import styles from './BotList.module.css';

interface BotListProps {
  bots: Bot[];
  onUpdated: () => void;
}

export function BotList({ bots, onUpdated }: BotListProps) {
  if (bots.length === 0) {
    return <p className={styles.empty}>No bots yet. Create one from the Marketplace tab.</p>;
  }

  return (
    <div className={styles.grid}>
      {bots.map((bot) => (
        <BotCard key={bot.bot_id} bot={bot} onUpdated={onUpdated} />
      ))}
    </div>
  );
}
